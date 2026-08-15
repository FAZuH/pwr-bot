//! Interaction engine: routes Discord component interactions to plugin view
//! sessions and back.
//!
//! A plugin slash command opens a session for the message it renders
//! ([`InteractionEngine::open`]), keyed by the message id. Every later
//! interaction with that message routes to the plugin via a `view.interact`
//! call carrying the raw Discord interaction, the pressed `custom_id`, and
//! the plugin's own opaque view state ([`interact_args`]). The plugin answers
//! with the view's next spec — rendered by the caller verbatim — or a
//! first-class wire error, surfaced as [`InteractionError::PluginRejected`].
//!
//! Success payloads come in two shapes, both accepted here:
//! - the full envelope `{"data": ..., "ephemeral": ..., "view": ...}`, used
//!   verbatim (recognized by a top-level `data` key);
//! - the v1 raw shape: the Discord message JSON itself, wrapped as
//!   `{data, ephemeral: false, view: <stored view>}`. The canonical fixture
//!   serves this shape today.
//!
//! Sessions end with [`InteractionEngine::abandon`], which pushes a one-way
//! `view.timeout` event (the session is dropped regardless of whether the
//! push succeeds) so the plugin can expire its own state.
//!
//! The engine is generic over [`PluginHandle`] so it can be unit-tested
//! against an in-memory fake; [`RunningPlugin`] implements the trait for real
//! subprocesses. Message ids are [`serenity::MessageId`]s.
//!
//! Rendering seam: this module never touches Discord. `open`/`interact`
//! return the [`ViewSpec`] and the caller renders `spec.data` verbatim
//! (e.g. via `ctx.send`/`ctx.edit`); hooking component interactions into
//! [`InteractionEngine::interact`] is the #109 registry seam.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use log::warn;
use poise::serenity_prelude as serenity;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::ViewSpec;
use pwr_plugin_protocol::WireError;
use serde_json::Map;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::plugin::PluginError;
use crate::plugin::RunningPlugin;

/// How the interaction engine talks to a plugin: correlated calls plus
/// one-way event pushes. Implemented by [`RunningPlugin`] (and `Arc<P>`
/// wrappers) for real subprocesses; tests use an in-memory fake.
#[async_trait]
pub trait PluginHandle: Send + Sync {
    /// Sends a `call` and awaits the correlated `resp`.
    async fn call(
        &self,
        op: &str,
        cmd: Option<&str>,
        args: Option<Value>,
    ) -> Result<Msg, PluginError>;

    /// Pushes a one-way event (e.g. `view.timeout`) without awaiting a reply.
    async fn send_event(&self, name: &str, data: Option<Value>) -> Result<(), PluginError>;
}

#[async_trait]
impl PluginHandle for RunningPlugin {
    async fn call(
        &self,
        op: &str,
        cmd: Option<&str>,
        args: Option<Value>,
    ) -> Result<Msg, PluginError> {
        RunningPlugin::call(self, op, cmd, args).await
    }

    async fn send_event(&self, name: &str, data: Option<Value>) -> Result<(), PluginError> {
        RunningPlugin::send_event(self, name, data).await
    }
}

#[async_trait]
impl<P: PluginHandle + ?Sized> PluginHandle for Arc<P> {
    async fn call(
        &self,
        op: &str,
        cmd: Option<&str>,
        args: Option<Value>,
    ) -> Result<Msg, PluginError> {
        self.as_ref().call(op, cmd, args).await
    }

    async fn send_event(&self, name: &str, data: Option<Value>) -> Result<(), PluginError> {
        self.as_ref().send_event(name, data).await
    }
}

/// A live view session: one plugin-rendered message being interacted with.
///
/// `generation` and `lock` are session identity and serialization: every
/// [`InteractionEngine::open`] installs a fresh pair, so an in-flight
/// [`InteractionEngine::interact`] against a replaced session can both detect
/// the replacement (generation) and hold the old session's serialization
/// guard without blocking the new one.
#[derive(Debug)]
struct Session<P> {
    /// The plugin owning the view.
    plugin: Arc<P>,
    /// Command name the session was opened from.
    command: String,
    /// Opaque plugin view state, handed back verbatim on each interaction.
    view: Value,
    /// Bumped on every open-replace; guards stale write-backs against a
    /// re-opened session.
    generation: u64,
    /// Serializes interacts on one session; one fresh lock per open.
    lock: Arc<Mutex<()>>,
}

/// A read-only copy of a session, taken under the sessions lock so callers
/// can work with it outside the lock.
#[derive(Debug)]
struct SessionSnapshot<P> {
    plugin: Arc<P>,
    command: String,
    view: Value,
    generation: u64,
}

/// An error from the interaction engine.
#[derive(Debug, thiserror::Error)]
pub enum InteractionError {
    /// No session is open for the given message id.
    #[error("no plugin session for message {message_id}")]
    NoSession {
        /// The message id the interaction targeted.
        message_id: serenity::MessageId,
    },

    /// The underlying plugin call or event push failed.
    #[error(transparent)]
    Plugin(#[from] PluginError),

    /// The plugin answered a call with a first-class wire error.
    #[error("plugin rejected the interaction ({kind}): {msg}")]
    PluginRejected {
        /// Wire error kind, e.g. `UnknownAction`.
        kind: String,
        /// Human-readable wire error message.
        msg: String,
    },

    /// The plugin answered with something other than the correlated `resp`
    /// the wire contract requires.
    #[error("plugin answered with an unexpected reply: {detail}")]
    UnexpectedReply {
        /// What the plugin sent instead of a resp.
        detail: String,
    },
}

/// Routes Discord interactions to plugin view sessions keyed by message id.
#[derive(Debug)]
pub struct InteractionEngine<P> {
    inner: Arc<EngineInner<P>>,
}

impl<P> Clone for InteractionEngine<P> {
    /// Clones the handle to the same engine state; sessions are shared.
    fn clone(&self) -> Self {
        InteractionEngine {
            inner: self.inner.clone(),
        }
    }
}

#[derive(Debug)]
struct EngineInner<P> {
    sessions: Mutex<HashMap<serenity::MessageId, Session<P>>>,
    /// Source of per-session generation tokens; each open-replace takes a
    /// fresh value so old sessions can be told apart from their successors.
    next_generation: AtomicU64,
}

impl<P> InteractionEngine<P> {
    /// A new engine with no open sessions.
    pub fn new() -> Self {
        InteractionEngine {
            inner: Arc::new(EngineInner {
                sessions: Mutex::new(HashMap::new()),
                next_generation: AtomicU64::new(0),
            }),
        }
    }

    /// Whether a session is open for `message_id`.
    pub async fn has_session(&self, message_id: serenity::MessageId) -> bool {
        self.inner.sessions.lock().await.contains_key(&message_id)
    }

    /// The stored opaque view state of the session for `message_id`, if any.
    pub async fn view_state(&self, message_id: serenity::MessageId) -> Option<Value> {
        self.inner
            .sessions
            .lock()
            .await
            .get(&message_id)
            .map(|session| session.view.clone())
    }
}

impl<P: PluginHandle> InteractionEngine<P> {
    /// Opens a session for a plugin view: invokes the plugin's command and
    /// stores the returned spec's opaque `view` under `message_id`. Returns
    /// the [`ViewSpec`] the caller renders verbatim (typically via
    /// `ctx.send`). Any session already open for `message_id` is replaced.
    pub async fn open(
        &self,
        message_id: serenity::MessageId,
        plugin: Arc<P>,
        command: &str,
        args: Value,
    ) -> Result<ViewSpec, InteractionError> {
        let resp = plugin.call("invoke", Some(command), Some(args)).await?;
        let spec = view_spec_from_resp(resp, None)?;
        let generation = self.inner.next_generation.fetch_add(1, Ordering::SeqCst);
        self.inner.sessions.lock().await.insert(
            message_id,
            Session {
                plugin,
                command: command.to_string(),
                view: spec.view.clone(),
                generation,
                lock: Arc::new(Mutex::new(())),
            },
        );
        Ok(spec)
    }

    /// Routes one component interaction to the session's plugin: the stored
    /// `view` state and the pressed `custom_id` ride along in the call args
    /// ([`interact_args`]), and the returned spec's `view` becomes the
    /// session's new state. Errors with [`InteractionError::NoSession`] when
    /// `message_id` has no open session.
    ///
    /// Interacts on one session are serialized: a per-session lock is held
    /// for the whole round trip, so concurrent clicks see each other's
    /// committed view state and no update is lost. A concurrent `open` may
    /// replace the session mid-call; the stale interact then returns
    /// [`InteractionError::NoSession`] (if it was still queued) or resolves
    /// normally without writing its view back over the new session.
    pub async fn interact(
        &self,
        message_id: serenity::MessageId,
        custom_id: &str,
        interaction: Value,
    ) -> Result<ViewSpec, InteractionError> {
        // Take the session's serialization lock first: interacts on one
        // session run one at a time, so each sees the previous one's
        // committed view.
        let (lock, generation) = {
            let sessions = self.inner.sessions.lock().await;
            let session = sessions
                .get(&message_id)
                .ok_or(InteractionError::NoSession { message_id })?;
            (session.lock.clone(), session.generation)
        };
        let _guard = lock.lock().await;

        // Re-read the session under the lock: a concurrent `open` may have
        // replaced it while we waited, and an earlier interact on this
        // session committed a fresh view.
        let snapshot = self.session_snapshot(message_id).await?;
        if snapshot.generation != generation {
            // The click targets a session that was replaced while the
            // interact was queued; treat it as gone.
            return Err(InteractionError::NoSession { message_id });
        }

        let args = interact_args(&snapshot.view, custom_id, interaction);
        let resp = snapshot
            .plugin
            .call("view.interact", Some(snapshot.command.as_str()), Some(args))
            .await?;
        let spec = view_spec_from_resp(resp, Some(snapshot.view))?;

        // Write back only to the same session we interacted with: a
        // concurrent open-replace must not be clobbered by a stale in-flight
        // interact. The session lock is still held, so no other interact on
        // this session can commit in between.
        if let Some(session) = self.inner.sessions.lock().await.get_mut(&message_id)
            && session.generation == snapshot.generation
        {
            session.view = spec.view.clone();
        }
        Ok(spec)
    }

    /// Abandons the session for `message_id`: pushes a one-way `view.timeout`
    /// event to the plugin and drops the session. A failed push (e.g. the
    /// plugin died) is logged, not fatal — the session is abandoned either
    /// way. Errors only with [`InteractionError::NoSession`].
    pub async fn abandon(&self, message_id: serenity::MessageId) -> Result<(), InteractionError> {
        let snapshot = self.session_snapshot(message_id).await?;
        if let Err(e) = snapshot
            .plugin
            .send_event("view.timeout", Some(snapshot.view))
            .await
        {
            warn!("failed to push view.timeout for message {message_id}: {e}");
        }
        self.inner.sessions.lock().await.remove(&message_id);
        Ok(())
    }

    /// Snapshots the session for `message_id` under the sessions lock, for
    /// working with it outside the lock. Errors with
    /// [`InteractionError::NoSession`] when no session is open.
    async fn session_snapshot(
        &self,
        message_id: serenity::MessageId,
    ) -> Result<SessionSnapshot<P>, InteractionError> {
        let sessions = self.inner.sessions.lock().await;
        let session = sessions
            .get(&message_id)
            .ok_or(InteractionError::NoSession { message_id })?;
        Ok(SessionSnapshot {
            plugin: session.plugin.clone(),
            command: session.command.clone(),
            view: session.view.clone(),
            generation: session.generation,
        })
    }
}

/// Interprets a call response as a [`ViewSpec`]. A failed resp becomes
/// [`InteractionError::PluginRejected`]; anything that is not a resp — or a
/// success without a payload — is [`InteractionError::UnexpectedReply`].
fn view_spec_from_resp(
    resp: Msg,
    current_view: Option<Value>,
) -> Result<ViewSpec, InteractionError> {
    match resp {
        Msg::Resp {
            ok: true,
            data: Some(data),
            ..
        } => Ok(view_spec_from_data(data, current_view)),
        Msg::Resp {
            ok: true,
            data: None,
            ..
        } => Err(InteractionError::UnexpectedReply {
            detail: "successful resp without a data payload".into(),
        }),
        Msg::Resp {
            ok: false, error, ..
        } => {
            let error = error.unwrap_or_else(|| WireError {
                kind: "Unknown".into(),
                msg: "ok:false without an error".into(),
            });
            Err(InteractionError::PluginRejected {
                kind: error.kind,
                msg: error.msg,
            })
        }
        other => Err(InteractionError::UnexpectedReply {
            detail: format!("{other:?}"),
        }),
    }
}

/// Interprets a success payload as a [`ViewSpec`]. A payload shaped like the
/// full envelope (`{data, ephemeral, view}`) is used verbatim; anything else
/// is the v1 raw shape (Discord message JSON) and is wrapped with defaults,
/// carrying the session's stored `view` through unchanged. `data` is not a
/// top-level Discord message field, so the `data`-key check is unambiguous.
fn view_spec_from_data(data: Value, current_view: Option<Value>) -> ViewSpec {
    match data {
        Value::Object(map) if map.contains_key("data") => ViewSpec {
            data: map.get("data").cloned().unwrap_or_default(),
            ephemeral: map
                .get("ephemeral")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            view: map
                .get("view")
                .cloned()
                .or_else(|| current_view.clone())
                .unwrap_or_default(),
        },
        data => ViewSpec {
            data,
            ephemeral: false,
            view: current_view.unwrap_or_default(),
        },
    }
}

/// Builds the `view.interact` call args: the raw interaction merged with the
/// pressed `custom_id` (verbatim) and the session's opaque `view` state.
fn interact_args(view: &Value, custom_id: &str, interaction: Value) -> Value {
    let mut map = match interaction {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    map.insert("custom_id".into(), Value::String(custom_id.to_string()));
    map.insert("view".into(), view.clone());
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use pwr_plugin_protocol::BUTTON_CUSTOM_ID;
    use pwr_plugin_protocol::Msg;
    use pwr_plugin_protocol::ViewSpec;
    use pwr_plugin_protocol::WireError;
    use serde_json::json;
    use tokio::sync::mpsc;

    use super::*;

    /// In-memory plugin double: answers `invoke`/`view.interact` from an
    /// atomic click counter and records events, so the engine can be tested
    /// without a subprocess.
    #[derive(Debug, Default)]
    struct FakePlugin {
        clicks: AtomicU64,
        events: StdMutex<Vec<(String, Option<Value>)>>,
        last_args: StdMutex<Option<Value>>,
        wrong_reply: AtomicBool,
        dead_events: AtomicBool,
        /// While `gate_active` is set, each `view.interact` call waits for one
        /// message on `gate` before answering — tests use this to hold a call
        /// in flight while the engine is exercised from other tasks.
        gate: Mutex<Option<mpsc::Receiver<()>>>,
        gate_active: AtomicBool,
        /// How many calls are currently waiting on the gate.
        gate_entries: AtomicU64,
    }

    #[async_trait]
    impl PluginHandle for FakePlugin {
        async fn call(
            &self,
            op: &str,
            _cmd: Option<&str>,
            args: Option<Value>,
        ) -> Result<Msg, PluginError> {
            if self.wrong_reply.load(Ordering::SeqCst) {
                return Ok(Msg::Pong);
            }
            self.last_args.lock().unwrap().clone_from(&args);
            match op {
                "invoke" => Ok(Msg::resp_ok(0, Some(json!({"content": "hello"})))),
                "view.interact" => {
                    if self.gate_active.load(Ordering::SeqCst)
                        && let Some(rx) = self.gate.lock().await.as_mut()
                    {
                        self.gate_entries.fetch_add(1, Ordering::SeqCst);
                        let _ = rx.recv().await;
                    }
                    let custom_id = args
                        .as_ref()
                        .and_then(|a| a.get("custom_id"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if custom_id != BUTTON_CUSTOM_ID {
                        return Ok(Msg::resp_err(
                            0,
                            WireError {
                                kind: "UnknownAction".into(),
                                msg: "unknown custom_id".into(),
                            },
                        ));
                    }
                    let clicks = self.clicks.fetch_add(1, Ordering::SeqCst) + 1;
                    Ok(Msg::resp_ok(
                        0,
                        Some(json!({
                            "data": {"content": format!("count={clicks}")},
                            "ephemeral": false,
                            "view": {"clicks": clicks},
                        })),
                    ))
                }
                _ => unreachable!("unexpected op {op}"),
            }
        }

        async fn send_event(&self, name: &str, data: Option<Value>) -> Result<(), PluginError> {
            if self.dead_events.load(Ordering::SeqCst) {
                return Err(PluginError::PluginDied {
                    name: "fake".into(),
                });
            }
            self.events.lock().unwrap().push((name.to_string(), data));
            Ok(())
        }
    }

    /// Opens a session on `engine` and returns its message id.
    async fn opened(
        engine: &InteractionEngine<FakePlugin>,
        plugin: Arc<FakePlugin>,
    ) -> serenity::MessageId {
        let id = serenity::MessageId::new(7);
        engine
            .open(id, plugin, "hello", json!({}))
            .await
            .expect("open a session");
        id
    }

    /// Parses the click count out of a spec's content string.
    fn click_count(spec: &ViewSpec) -> u64 {
        spec.data["content"]
            .as_str()
            .expect("content string")
            .split("count=")
            .nth(1)
            .expect("count present")
            .parse()
            .expect("count parses")
    }

    /// Polls until at least one call is waiting on the fake's gate.
    async fn wait_for_gate_entry(plugin: &FakePlugin) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while plugin.gate_entries.load(Ordering::SeqCst) == 0 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the fake plugin never entered the gate"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    // ── wire shapes ──────────────────────────────────────────────────────────

    #[test]
    fn view_spec_envelope_resp_is_rendered_verbatim() {
        let resp = Msg::resp_ok(
            0,
            Some(json!({
                "data": {"content": "hello"},
                "ephemeral": true,
                "view": {"page": 2},
            })),
        );
        let spec = view_spec_from_resp(resp, Some(json!({"page": 1}))).unwrap();
        assert_eq!(spec.data, json!({"content": "hello"}));
        assert!(spec.ephemeral);
        assert_eq!(spec.view, json!({"page": 2}));
    }

    #[test]
    fn raw_data_resp_is_wrapped_with_defaults() {
        let resp = Msg::resp_ok(0, Some(json!({"content": "hello"})));
        let spec = view_spec_from_resp(resp, Some(json!({"page": 1}))).unwrap();
        assert_eq!(spec.data, json!({"content": "hello"}));
        assert!(!spec.ephemeral);
        assert_eq!(
            spec.view,
            json!({"page": 1}),
            "raw shape keeps the stored view"
        );
    }

    #[test]
    fn envelope_without_view_keeps_the_stored_view() {
        let resp = Msg::resp_ok(
            0,
            Some(json!({
                "data": {"content": "hello"},
                "ephemeral": true,
            })),
        );
        let spec = view_spec_from_resp(resp, Some(json!({"page": 1}))).unwrap();
        assert_eq!(spec.data, json!({"content": "hello"}));
        assert!(spec.ephemeral);
        assert_eq!(
            spec.view,
            json!({"page": 1}),
            "an envelope omitting view keeps the stored view"
        );
    }

    #[test]
    fn failed_resp_becomes_plugin_rejected() {
        let resp = Msg::resp_err(
            0,
            WireError {
                kind: "UnknownAction".into(),
                msg: "unknown custom_id".into(),
            },
        );
        let err = view_spec_from_resp(resp, None).unwrap_err();
        assert!(matches!(
            err,
            InteractionError::PluginRejected { kind, msg }
                if kind == "UnknownAction" && msg == "unknown custom_id"
        ));
    }

    #[test]
    fn ok_without_data_is_an_unexpected_reply_error() {
        let err = view_spec_from_resp(Msg::resp_ok(0, None), None).unwrap_err();
        assert!(matches!(err, InteractionError::UnexpectedReply { .. }));
    }

    #[test]
    fn non_resp_reply_is_an_unexpected_reply_error() {
        let err = view_spec_from_resp(Msg::Pong, None).unwrap_err();
        assert!(matches!(err, InteractionError::UnexpectedReply { .. }));
    }

    // ── session lifecycle ────────────────────────────────────────────────────

    #[tokio::test]
    async fn invoke_opens_a_session_and_returns_the_view_spec() {
        let plugin = Arc::new(FakePlugin::default());
        let engine = InteractionEngine::new();
        let id = opened(&engine, plugin).await;

        assert!(engine.has_session(id).await);
        assert_eq!(engine.view_state(id).await, Some(Value::Null));
    }

    #[tokio::test]
    async fn interact_round_trips_and_updates_the_stored_view() {
        let plugin = Arc::new(FakePlugin::default());
        let engine = InteractionEngine::new();
        let id = opened(&engine, plugin).await;

        let spec = engine
            .interact(id, BUTTON_CUSTOM_ID, json!({"user_id": 1}))
            .await
            .expect("first click");
        assert_eq!(spec.data["content"], "count=1");
        assert_eq!(
            engine.view_state(id).await,
            Some(json!({"clicks": 1})),
            "the envelope view becomes the session state"
        );

        let spec = engine
            .interact(id, BUTTON_CUSTOM_ID, json!({"user_id": 1}))
            .await
            .expect("second click");
        assert_eq!(spec.data["content"], "count=2");
        assert_eq!(engine.view_state(id).await, Some(json!({"clicks": 2})));
    }

    #[tokio::test]
    async fn interact_with_unknown_message_id_errors() {
        let engine = InteractionEngine::<FakePlugin>::new();
        let err = engine
            .interact(serenity::MessageId::new(1), BUTTON_CUSTOM_ID, json!({}))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            InteractionError::NoSession { message_id }
                if message_id == serenity::MessageId::new(1)
        ));
    }

    #[tokio::test]
    async fn interact_with_unknown_custom_id_surfaces_the_plugin_error() {
        let plugin = Arc::new(FakePlugin::default());
        let engine = InteractionEngine::new();
        let id = opened(&engine, plugin).await;

        let err = engine
            .interact(id, "hello:nope", json!({}))
            .await
            .unwrap_err();
        assert!(
            matches!(
                &err,
                InteractionError::PluginRejected { kind, msg }
                    if kind == "UnknownAction" && msg == "unknown custom_id"
            ),
            "expected a plugin rejection, got {err:?}"
        );
        assert!(
            engine.has_session(id).await,
            "a rejected interaction keeps the session"
        );
    }

    #[tokio::test]
    async fn interact_passes_the_custom_id_and_stored_view_through() {
        let plugin = Arc::new(FakePlugin::default());
        let engine = InteractionEngine::new();
        let id = opened(&engine, plugin.clone()).await;

        engine
            .interact(id, BUTTON_CUSTOM_ID, json!({}))
            .await
            .expect("first click stores the view");
        engine
            .interact(id, BUTTON_CUSTOM_ID, json!({"user_id": 9}))
            .await
            .expect("second click");
        let args = plugin
            .last_args
            .lock()
            .unwrap()
            .clone()
            .expect("args recorded");
        assert_eq!(args["custom_id"], BUTTON_CUSTOM_ID);
        assert_eq!(
            args["view"],
            json!({"clicks": 1}),
            "stored view rides along"
        );
        assert_eq!(
            args["user_id"].as_u64(),
            Some(9),
            "the raw interaction is merged through"
        );
    }

    #[tokio::test]
    async fn abandon_pushes_view_timeout_and_drops_the_session() {
        let plugin = Arc::new(FakePlugin::default());
        let engine = InteractionEngine::new();
        let id = opened(&engine, plugin.clone()).await;

        engine.abandon(id).await.expect("abandon");
        assert!(!engine.has_session(id).await);
        let events = plugin.events.lock().unwrap().clone();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "view.timeout");
        assert_eq!(
            events[0].1,
            Some(Value::Null),
            "the stored view rides along"
        );
    }

    #[tokio::test]
    async fn abandon_with_unknown_message_id_errors() {
        let engine = InteractionEngine::<FakePlugin>::new();
        let err = engine
            .abandon(serenity::MessageId::new(1))
            .await
            .unwrap_err();
        assert!(matches!(err, InteractionError::NoSession { .. }));
    }

    #[tokio::test]
    async fn abandon_with_a_dead_plugin_is_graceful() {
        let plugin = Arc::new(FakePlugin::default());
        plugin.dead_events.store(true, Ordering::SeqCst);
        let engine = InteractionEngine::new();
        let id = opened(&engine, plugin.clone()).await;

        engine
            .abandon(id)
            .await
            .expect("abandon despite a dead plugin");
        assert!(!engine.has_session(id).await);
        assert!(plugin.events.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn sessions_keep_independent_view_state() {
        let plugin = Arc::new(FakePlugin::default());
        let engine = InteractionEngine::new();
        let a = serenity::MessageId::new(1);
        let b = serenity::MessageId::new(2);
        engine
            .open(a, plugin.clone(), "hello", json!({}))
            .await
            .expect("open a");
        engine
            .open(b, plugin.clone(), "hello", json!({}))
            .await
            .expect("open b");

        engine
            .interact(a, BUTTON_CUSTOM_ID, json!({}))
            .await
            .expect("click a");
        engine
            .interact(b, BUTTON_CUSTOM_ID, json!({}))
            .await
            .expect("click b");

        assert_eq!(engine.view_state(a).await, Some(json!({"clicks": 1})));
        assert_eq!(engine.view_state(b).await, Some(json!({"clicks": 2})));
    }

    #[tokio::test]
    async fn concurrent_interactions_on_different_sessions_do_not_collide() {
        let plugin = Arc::new(FakePlugin::default());
        let engine = InteractionEngine::new();
        let a = serenity::MessageId::new(1);
        let b = serenity::MessageId::new(2);
        engine
            .open(a, plugin.clone(), "hello", json!({}))
            .await
            .expect("open a");
        engine
            .open(b, plugin.clone(), "hello", json!({}))
            .await
            .expect("open b");

        let (ra, rb) = tokio::join!(
            engine.interact(a, BUTTON_CUSTOM_ID, json!({})),
            engine.interact(b, BUTTON_CUSTOM_ID, json!({})),
        );
        let sa = ra.expect("click a resolves");
        let sb = rb.expect("click b resolves");

        // Each session stores exactly the view its own interact returned; a
        // cross-session write-back would fail these.
        assert_eq!(engine.view_state(a).await, Some(sa.view.clone()));
        assert_eq!(engine.view_state(b).await, Some(sb.view.clone()));
        assert_ne!(sa.view, sb.view, "the two clicks land in distinct sessions");

        let mut counts = vec![
            sa.view["clicks"].as_u64().expect("clicks number"),
            sb.view["clicks"].as_u64().expect("clicks number"),
        ];
        counts.sort_unstable();
        assert_eq!(counts, vec![1, 2]);
    }

    #[tokio::test]
    async fn concurrent_interacts_on_one_session_are_serialized() {
        let (gate_tx, gate_rx) = mpsc::channel(1);
        let plugin = Arc::new(FakePlugin {
            gate_active: AtomicBool::new(true),
            gate: Mutex::new(Some(gate_rx)),
            ..Default::default()
        });
        let engine = InteractionEngine::new();
        let id = opened(&engine, plugin.clone()).await;

        let i1 = tokio::spawn({
            let engine = engine.clone();
            async move { engine.interact(id, BUTTON_CUSTOM_ID, json!({})).await }
        });
        // Pin the first click inside the plugin call before the second is
        // spawned, so the second one can only ever see the first one's
        // committed view (interacts on one session are serialized).
        wait_for_gate_entry(&plugin).await;
        let i2 = tokio::spawn({
            let engine = engine.clone();
            async move { engine.interact(id, BUTTON_CUSTOM_ID, json!({})).await }
        });

        // Release each plugin call as it arrives; whichever lands second must
        // see the first one's committed view, so no update is lost.
        gate_tx.send(()).await.expect("release first call");
        gate_tx.send(()).await.expect("release second call");

        let s1 = i1.await.expect("task").expect("first click");
        let s2 = i2.await.expect("task").expect("second click");
        let mut counts = [click_count(&s1), click_count(&s2)];
        counts.sort_unstable();
        assert_eq!(counts, [1, 2], "each click is counted once");
        assert_eq!(
            engine.view_state(id).await,
            Some(json!({"clicks": 2})),
            "both clicks land in the session state"
        );
        let args = plugin
            .last_args
            .lock()
            .unwrap()
            .clone()
            .expect("args recorded");
        assert_eq!(
            args["view"],
            json!({"clicks": 1}),
            "the later click carried the earlier click's committed view"
        );
    }

    #[tokio::test]
    async fn stale_interact_does_not_clobber_a_replaced_session() {
        let (gate_tx, gate_rx) = mpsc::channel(1);
        let plugin = Arc::new(FakePlugin {
            gate: Mutex::new(Some(gate_rx)),
            ..Default::default()
        });
        let engine = InteractionEngine::new();
        let id = serenity::MessageId::new(7);
        engine
            .open(id, plugin.clone(), "hello", json!({}))
            .await
            .expect("open");
        engine
            .interact(id, BUTTON_CUSTOM_ID, json!({}))
            .await
            .expect("seed the session view");
        assert_eq!(engine.view_state(id).await, Some(json!({"clicks": 1})));

        // A second click blocks inside the plugin call, holding the session.
        plugin.gate_active.store(true, Ordering::SeqCst);
        let stale = tokio::spawn({
            let engine = engine.clone();
            async move { engine.interact(id, BUTTON_CUSTOM_ID, json!({})).await }
        });
        wait_for_gate_entry(&plugin).await;

        // Re-open the same message: the session is replaced while the stale
        // interact is still in flight.
        engine
            .open(id, plugin.clone(), "hello", json!({}))
            .await
            .expect("reopen replaces the session");
        assert_eq!(engine.view_state(id).await, Some(Value::Null));

        gate_tx.send(()).await.expect("release the stale call");
        let spec = stale.await.expect("task").expect("stale click resolves");
        assert!(
            spec.data["content"]
                .as_str()
                .expect("content")
                .contains("count=2")
        );
        assert_eq!(
            engine.view_state(id).await,
            Some(Value::Null),
            "a stale in-flight interact does not clobber a re-opened session"
        );
    }

    #[tokio::test]
    async fn interact_queued_on_a_replaced_session_errors() {
        let (gate_tx, gate_rx) = mpsc::channel(1);
        let plugin = Arc::new(FakePlugin {
            gate_active: AtomicBool::new(true),
            gate: Mutex::new(Some(gate_rx)),
            ..Default::default()
        });
        let engine = InteractionEngine::new();
        let id = serenity::MessageId::new(7);
        engine
            .open(id, plugin.clone(), "hello", json!({}))
            .await
            .expect("open");

        // First interact holds the session lock, blocked inside the plugin.
        let holder = tokio::spawn({
            let engine = engine.clone();
            async move { engine.interact(id, BUTTON_CUSTOM_ID, json!({})).await }
        });
        wait_for_gate_entry(&plugin).await;

        // Second interact queues on the session lock with the old identity.
        let queued = tokio::spawn({
            let engine = engine.clone();
            async move { engine.interact(id, BUTTON_CUSTOM_ID, json!({})).await }
        });
        // Let the queued interact reach its initial session lookup before
        // the session is replaced underneath it.
        tokio::task::yield_now().await;

        engine
            .open(id, plugin.clone(), "hello", json!({}))
            .await
            .expect("reopen replaces the session");

        gate_tx.send(()).await.expect("release the holder");
        let spec = holder.await.expect("task").expect("holder click resolves");
        assert_eq!(click_count(&spec), 1, "the holder's click is counted once");
        let err = queued.await.expect("task").unwrap_err();
        assert!(
            matches!(err, InteractionError::NoSession { message_id } if message_id == id),
            "a click queued on a replaced session sees NoSession, got {err:?}"
        );
        assert_eq!(
            engine.view_state(id).await,
            Some(Value::Null),
            "the reopened session's view survives both interacts"
        );
    }

    #[tokio::test]
    async fn open_replaces_an_existing_session() {
        let plugin = Arc::new(FakePlugin::default());
        let engine = InteractionEngine::new();
        let id = serenity::MessageId::new(7);
        engine
            .open(id, plugin.clone(), "hello", json!({}))
            .await
            .expect("first open");
        engine
            .interact(id, BUTTON_CUSTOM_ID, json!({}))
            .await
            .expect("click");
        assert_eq!(engine.view_state(id).await, Some(json!({"clicks": 1})));

        engine
            .open(id, plugin.clone(), "hello", json!({}))
            .await
            .expect("reopen replaces the session");
        assert_eq!(
            engine.view_state(id).await,
            Some(Value::Null),
            "the reopened session starts from the fresh spec's view"
        );
        let spec = engine
            .interact(id, BUTTON_CUSTOM_ID, json!({}))
            .await
            .expect("click after reopen");
        assert_eq!(spec.data["content"], "count=2");
        assert_eq!(
            engine.view_state(id).await,
            Some(json!({"clicks": 2})),
            "the reopened session is interactive on its own state"
        );
    }

    #[tokio::test]
    async fn wrong_reply_is_an_unexpected_reply_error() {
        let plugin = Arc::new(FakePlugin::default());
        let engine = InteractionEngine::new();
        let id = opened(&engine, plugin.clone()).await;
        plugin.wrong_reply.store(true, Ordering::SeqCst);

        let err = engine
            .interact(id, BUTTON_CUSTOM_ID, json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, InteractionError::UnexpectedReply { .. }));
    }
}
