//! Author-keyed modal routing: binds a `host.open_modal` call to its owner
//! and delivers the later submission to exactly that plugin session.
//!
//! Discord's modal collector is author-keyed, not message-keyed: any modal
//! the author submits in the collector's window matches, regardless of which
//! message the modal was opened from (ADR-0011). A submission therefore
//! cannot be routed by its message alone. When a plugin opens a modal, the
//! host records the author of the triggering interaction and the owning
//! plugin; when the author submits, the bot's modal-submit handler trades
//! the binding for the raw submission and the owner answers with the view
//! rendered as the submission's response — the same view-spec contract a
//! `view.interact` answer follows.
//!
//! The binding is one-shot and never hangs: delivering a submission consumes
//! it, so a second submission from the same author (without a fresh
//! `host.open_modal`) finds no route and falls back to the message-keyed
//! view route. A binding expires with the interaction window
//! ([`DEFAULT_VIEW_TIMEOUT`]), and unloading the owner plugin drops its
//! bindings (session death ends the route). A missing or expired route is a
//! typed [`ModalRouteError`], never a hang.

use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use log::warn;
use pwr_plugin_protocol::MODAL_SUBMIT_OP;
use pwr_plugin_protocol::ViewSpec;
use serde_json::Map;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::plugin::PluginError;
use crate::plugin::PluginManager;
use crate::plugin::interaction::view_spec_from_resp;
use crate::plugin::validate_view_data;

/// One author's live modal route: the plugin that opened the modal and the
/// custom_id of the open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalBinding {
    /// The plugin session that owns the modal (its manager-registered name).
    pub owner: String,
    /// The custom_id of the opened modal, as the plugin authored it.
    pub custom_id: String,
    /// When the modal was opened; [`ModalRouter::take`] drops a binding once
    /// the router's window has passed.
    opened_at: Instant,
}

/// Why a modal submission has no owner route.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModalRouteError {
    /// No plugin modal is open for this author.
    #[error("no plugin modal is open for author {0}")]
    NoBinding(u64),
    /// The author's modal route outlived the interaction window and was
    /// dropped.
    #[error("modal route for author {0} expired")]
    Expired(u64),
}

/// An error from delivering a modal submission to its owner.
#[derive(Debug, thiserror::Error)]
pub enum ModalDeliveryError {
    /// No (or no unexpired) owner route for the submitting author.
    #[error(transparent)]
    Route(#[from] ModalRouteError),
    /// The owning plugin is not running: unloaded or crashed since the open.
    #[error("modal owner plugin `{0}` is not running")]
    OwnerGone(String),
    /// The owner plugin call or its transport failed.
    #[error(transparent)]
    Plugin(#[from] PluginError),
    /// The owner answered the submission with a first-class wire error.
    #[error("plugin rejected the modal submission ({kind}): {msg}")]
    PluginRejected {
        /// Wire error kind, e.g. `UnknownAction`.
        kind: String,
        /// Human-readable wire error message.
        msg: String,
    },
    /// The owner answered with something other than the correlated `resp`
    /// the wire contract requires.
    #[error("plugin answered with an unexpected reply: {detail}")]
    UnexpectedReply {
        /// What the plugin sent instead of a resp.
        detail: String,
    },
    /// The owner's answer failed the view gate before it could be rendered.
    #[error("plugin returned an invalid modal response ({kind}): {msg}")]
    InvalidResponse {
        /// Wire error kind, e.g. `InvalidView`.
        kind: String,
        /// Human-readable error message.
        msg: String,
    },
}

/// The author-keyed route table for plugin-opened modals. Lives on the
/// [`PluginManager`], which owns the sessions the routes point at.
#[derive(Debug)]
pub struct ModalRouter {
    bindings: Mutex<HashMap<u64, ModalBinding>>,
    /// How long a binding stays routable, mirroring the view-session window.
    window: Duration,
}

impl ModalRouter {
    /// A router with the given binding window.
    pub fn new(window: Duration) -> Self {
        Self {
            bindings: Mutex::new(HashMap::new()),
            window,
        }
    }

    /// Records (or replaces) the route for `author`: the modal with
    /// `custom_id` belongs to plugin `owner`. The latest open wins, so an
    /// author always has at most one route — a submission reaches exactly
    /// one session.
    pub async fn bind(&self, author: u64, owner: &str, custom_id: &str) {
        self.bindings.lock().await.insert(
            author,
            ModalBinding {
                owner: owner.to_string(),
                custom_id: custom_id.to_string(),
                opened_at: Instant::now(),
            },
        );
    }

    /// Consumes the route for `author` (one-shot) and returns it. A missing
    /// route is [`ModalRouteError::NoBinding`]; a route older than the
    /// window is dropped and answered [`ModalRouteError::Expired`] — both
    /// typed, never a hang.
    pub async fn take(&self, author: u64) -> Result<ModalBinding, ModalRouteError> {
        let binding = self.bindings.lock().await.remove(&author);
        match binding {
            Some(binding) if binding.opened_at.elapsed() < self.window => Ok(binding),
            Some(_) => Err(ModalRouteError::Expired(author)),
            None => Err(ModalRouteError::NoBinding(author)),
        }
    }

    /// Drops every route owned by `owner` (session death ends the route).
    /// Returns how many bindings were dropped.
    pub async fn remove_owner(&self, owner: &str) -> usize {
        let mut bindings = self.bindings.lock().await;
        let dropped: Vec<u64> = bindings
            .iter()
            .filter(|(_, binding)| binding.owner == owner)
            .map(|(author, _)| *author)
            .collect();
        for author in &dropped {
            bindings.remove(author);
        }
        dropped.len()
    }
}

impl PluginManager {
    /// Records that `author`'s modal (`custom_id`) belongs to the plugin
    /// session `owner`. Called by the `host.open_modal` dispatch arm once
    /// the modal actually opened, so a failed open leaves no route.
    pub async fn bind_modal(&self, author: u64, owner: &str, custom_id: &str) {
        self.modals.bind(author, owner, custom_id).await;
    }

    /// Consumes the author's modal route without delivering anything.
    pub async fn take_modal(&self, author: u64) -> Result<ModalBinding, ModalRouteError> {
        self.modals.take(author).await
    }

    /// Delivers one raw Discord modal submission (the serialized
    /// [`poise::serenity_prelude::ModalInteraction`] JSON) to the owning
    /// plugin session as a `view.modal_submit` call and returns the view the
    /// plugin wants rendered as the submission's response.
    ///
    /// The route is consumed on the way in: exactly one submission is
    /// delivered per `host.open_modal`. The answer passes the view gate
    /// ([`validate_view_data`]) before it is returned; the caller renders it
    /// against the submission's own interaction token.
    pub async fn deliver_modal_submission(
        &self,
        author: u64,
        interaction: Value,
    ) -> Result<ViewSpec, ModalDeliveryError> {
        let binding = self.take_modal(author).await?;
        let Some(plugin) = self.get(&binding.owner).await else {
            return Err(ModalDeliveryError::OwnerGone(binding.owner));
        };
        let args = modal_submit_args(&binding.custom_id, interaction);
        let resp = plugin
            .call(MODAL_SUBMIT_OP, None, Some(args))
            .await
            .map_err(ModalDeliveryError::from)?;
        let spec = view_spec_from_resp(resp, None).map_err(|error| match error {
            crate::plugin::InteractionError::PluginRejected { kind, msg } => {
                ModalDeliveryError::PluginRejected { kind, msg }
            }
            crate::plugin::InteractionError::UnexpectedReply { detail } => {
                ModalDeliveryError::UnexpectedReply { detail }
            }
            crate::plugin::InteractionError::InvalidView { kind, msg } => {
                ModalDeliveryError::InvalidResponse { kind, msg }
            }
            other => ModalDeliveryError::UnexpectedReply {
                detail: other.to_string(),
            },
        })?;
        validate_view_data(&spec.data).map_err(|error| ModalDeliveryError::InvalidResponse {
            kind: "InvalidView".into(),
            msg: error.to_string(),
        })?;
        Ok(spec)
    }

    /// Drops the modal routes owned by `name`. Called on unload: a dead
    /// session must not receive submissions.
    pub(crate) async fn drop_modal_routes(&self, name: &str) {
        let dropped = self.modals.remove_owner(name).await;
        if dropped > 0 {
            warn!("dropped {dropped} modal route(s) of unloaded plugin `{name}`");
        }
    }
}

/// Builds the `view.modal_submit` call args: the raw submission with the
/// modal's `custom_id` hoisted to the top level — the same shape a
/// `view.interact` answer carries ([`interact_args`]), so a plugin reads
/// both interactions the same way.
fn modal_submit_args(custom_id: &str, interaction: Value) -> Value {
    let mut map = match interaction {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    map.insert("custom_id".into(), Value::String(custom_id.to_string()));
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    // ── bind / take ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn take_returns_the_bound_route() {
        let router = ModalRouter::new(Duration::from_secs(60));
        router.bind(7, "hello", "hello:modal").await;

        let binding = router.take(7).await.expect("route present");
        assert_eq!(binding.owner, "hello");
        assert_eq!(binding.custom_id, "hello:modal");
    }

    #[tokio::test]
    async fn take_without_a_binding_is_a_typed_no_binding_error() {
        let router = ModalRouter::new(Duration::from_secs(60));

        let error = router.take(7).await.unwrap_err();
        assert_eq!(error, ModalRouteError::NoBinding(7));
    }

    #[tokio::test]
    async fn take_consumes_the_route_so_a_second_submission_finds_none() {
        let router = ModalRouter::new(Duration::from_secs(60));
        router.bind(7, "hello", "hello:modal").await;
        router.take(7).await.expect("first delivery");

        let error = router.take(7).await.unwrap_err();
        assert_eq!(error, ModalRouteError::NoBinding(7));
    }

    #[tokio::test]
    async fn rebinding_replaces_the_previous_owner() {
        let router = ModalRouter::new(Duration::from_secs(60));
        router.bind(7, "one", "one:modal").await;
        router.bind(7, "two", "two:modal").await;

        let binding = router.take(7).await.expect("route present");
        assert_eq!(binding.owner, "two", "the latest open owns the route");
        assert_eq!(binding.custom_id, "two:modal");
    }

    // ── expiry ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn a_route_older_than_the_window_expires_and_is_dropped() {
        let router = ModalRouter::new(Duration::ZERO);
        router.bind(7, "hello", "hello:modal").await;
        tokio::time::sleep(Duration::from_millis(5)).await;

        let error = router.take(7).await.unwrap_err();
        assert_eq!(error, ModalRouteError::Expired(7));
        // The stale route is gone, not left to expire again.
        let error = router.take(7).await.unwrap_err();
        assert_eq!(error, ModalRouteError::NoBinding(7));
    }

    #[tokio::test]
    async fn a_fresh_route_survives_a_tiny_window_check() {
        let router = ModalRouter::new(Duration::from_secs(60));
        router.bind(7, "hello", "hello:modal").await;

        assert!(router.take(7).await.is_ok());
    }

    // ── session death ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn remove_owner_drops_only_that_owners_routes() {
        let router = ModalRouter::new(Duration::from_secs(60));
        router.bind(1, "hello", "hello:modal").await;
        router.bind(2, "other", "other:modal").await;
        router.bind(3, "hello", "hello:second").await;

        assert_eq!(router.remove_owner("hello").await, 2);

        assert_eq!(
            router.take(1).await.unwrap_err(),
            ModalRouteError::NoBinding(1)
        );
        assert_eq!(
            router.take(3).await.unwrap_err(),
            ModalRouteError::NoBinding(3)
        );
        let binding = router.take(2).await.expect("other plugin's route survives");
        assert_eq!(binding.owner, "other");
    }

    // ── delivery args ────────────────────────────────────────────────────────

    #[test]
    fn modal_submit_args_hoist_the_custom_id() {
        let args = modal_submit_args(
            "hello:modal",
            json!({
                "id": "999",
                "data": {"custom_id": "hello:modal", "components": []},
            }),
        );

        assert_eq!(args["custom_id"], "hello:modal");
        assert_eq!(args["data"]["custom_id"], "hello:modal");
        assert_eq!(args["id"], "999");
    }

    #[test]
    fn modal_submit_args_tolerate_a_non_object_submission() {
        let args = modal_submit_args("hello:modal", json!(null));

        assert_eq!(args["custom_id"], "hello:modal");
    }

    // ── manager-level delivery ────────────────────────────────────────────────

    use crate::plugin::RespawnPolicy;

    #[tokio::test]
    async fn delivering_without_a_route_is_a_typed_no_binding_error() {
        let manager = PluginManager::new(None, RespawnPolicy::default());

        let error = manager
            .deliver_modal_submission(7, json!({}))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ModalDeliveryError::Route(ModalRouteError::NoBinding(7))
        ));
    }

    #[tokio::test]
    async fn delivering_to_an_unregistered_owner_is_a_typed_owner_gone_error() {
        let manager = PluginManager::new(None, RespawnPolicy::default());
        // The binding exists (some plugin opened a modal) but no session is
        // registered under the owner name: the submission must fail with a
        // typed error, not a hang.
        manager.bind_modal(7, "ghost", "hello:modal").await;

        let error = manager
            .deliver_modal_submission(7, json!({}))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ModalDeliveryError::OwnerGone(ref owner) if owner == "ghost"
        ));
    }
}
