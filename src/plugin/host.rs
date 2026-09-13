//! Host capability ops: the `host.*` calls a plugin may make on the host.
//!
//! The plugin announces the ops it needs in its hello `caps`; the host serves
//! them as plugin→host [`Msg::Call`]s. All Discord I/O (defer, acknowledge,
//! send/edit message) lives behind the [`HostIo`] trait so tests can drive it
//! with a mock instead of a live gateway; plugin key-value storage lives
//! behind the [`KvStore`] trait with a Postgres-backed implementation.
//! `host.get_config` is pure data and never touches a seam.
//!
//! Ops not in the v1 surface (unknown `host.*` strings, and non-`host.*` ops
//! like `invoke`, which only ever travel host→plugin) answer
//! `UnknownOp`; and a missing service answers `HostUnavailable` /
//! `ConfigUnavailable` / `KvUnavailable` instead of panicking.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;
use log::debug;
use log::warn;
use mockall::automock;
use poise::serenity_prelude as serenity;
use pwr_ext::prelude::CreateModalDe;
use pwr_plugin_protocol::HostCap;
use pwr_plugin_protocol::HostStats;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::ServerSettings;
use pwr_plugin_protocol::WireError;
use serde_json::Value;
use serde_json::json;

use crate::plugin::InteractionEngine;
use crate::plugin::InteractionError;
use crate::plugin::PluginManager;
use crate::plugin::RunningPlugin;
use crate::plugin::edit_body_for_transport;
use crate::plugin::reject_content_on_edit;
use crate::plugin::validate_view_data;
use crate::service::error::ServiceError;
use crate::service::traits::FeedSubscriptionProvider;
use crate::service::traits::VoiceTracker;
/// The seam between plugin `host.*` ops and Discord. The real implementation
/// wraps [`serenity::Http`]; tests use the mockall mock generated from this
/// trait, so no plugin test ever touches a live gateway.
#[automock]
#[async_trait]
pub trait HostIo: Send + Sync {
    /// Defers the interaction response (long-running command): the user sees
    /// a loading state until the final response arrives.
    async fn defer(&self, interaction_id: u64, token: &str) -> Result<(), HostError>;

    /// Acknowledges the interaction without a visible reply, allowing the
    /// original response to be edited later.
    async fn acknowledge(&self, interaction_id: u64, token: &str) -> Result<(), HostError>;

    /// Sends a message to a channel. The prose renders as a text display
    /// inside a Components V2 envelope — the send seam has no legacy content
    /// path, so a content-beside-V2 payload (Discord error 50035) is
    /// unrepresentable here. Returns the created message's id.
    async fn send_message(
        &self,
        channel_id: u64,
        content: &str,
    ) -> Result<Option<Value>, HostError>;

    /// Edits a previously sent message in place. `data` is the edit body:
    /// only the fields it provides are applied, so partial shapes are legal.
    /// The shape rules enforced are the content gate
    /// ([`reject_content_on_edit`]) and, at every call site, the transport
    /// strip ([`edit_body_for_transport`]): a view envelope is built for
    /// sending, so create-only fields — `sticker_ids` above all, which the
    /// channel edit endpoint rejects with error 50080 even when the array
    /// is empty — are removed before the body reaches this seam. The
    /// implementation sends what it receives verbatim. `attachments` are the
    /// files the body's `attachments` declaration names (ADR-0012); an empty
    /// list sends no files, which is what a body declaring none must carry.
    /// Returns the edited message's id.
    async fn edit_message(
        &self,
        channel_id: u64,
        message_id: u64,
        data: Value,
        attachments: Vec<serenity::CreateAttachment<'static>>,
    ) -> Result<Option<Value>, HostError>;

    /// Opens a modal as the response to the interaction `interaction_id`:
    /// the modal IS the response (ADR-0007), so an already-acked or
    /// already-answered interaction fails. `modal` is the modal spec in the
    /// same JSON grammar a view spec uses; a spec the host cannot parse
    /// fails before anything is sent.
    async fn open_modal(
        &self,
        interaction_id: u64,
        token: &str,
        modal: Value,
    ) -> Result<(), HostError>;
}

/// The real [`HostIo`], backed by a shared [`serenity::Http`] client.
pub struct SerenityHostIo {
    http: Arc<serenity::Http>,
}

impl SerenityHostIo {
    /// Wraps an [`Arc`] of the bot's HTTP client.
    pub fn new(http: Arc<serenity::Http>) -> Self {
        Self { http }
    }
}

#[async_trait]
impl HostIo for SerenityHostIo {
    async fn defer(&self, interaction_id: u64, token: &str) -> Result<(), HostError> {
        serenity::CreateInteractionResponse::Defer(
            serenity::CreateInteractionResponseMessage::new(),
        )
        .execute(&self.http, interaction_id.into(), token)
        .await?;
        Ok(())
    }

    async fn acknowledge(&self, interaction_id: u64, token: &str) -> Result<(), HostError> {
        serenity::CreateInteractionResponse::Acknowledge
            .execute(&self.http, interaction_id.into(), token)
            .await?;
        Ok(())
    }

    async fn send_message(
        &self,
        channel_id: u64,
        content: &str,
    ) -> Result<Option<Value>, HostError> {
        let message = self
            .http
            .send_message(
                channel_id.into(),
                Vec::new(),
                &send_message_payload(content),
            )
            .await?;
        Ok(Some(json!({ "message_id": message.id.get() })))
    }

    async fn edit_message(
        &self,
        channel_id: u64,
        message_id: u64,
        data: Value,
        attachments: Vec<serenity::CreateAttachment<'static>>,
    ) -> Result<Option<Value>, HostError> {
        let message = self
            .http
            .edit_message(
                channel_id.into(),
                message_id.into(),
                &data,
                attachments.into_iter().map(Into::into).collect(),
            )
            .await?;
        Ok(Some(json!({ "message_id": message.id.get() })))
    }

    async fn open_modal(
        &self,
        interaction_id: u64,
        token: &str,
        modal: Value,
    ) -> Result<(), HostError> {
        let modal: serenity::CreateModal<'static> = serde_json::from_value::<CreateModalDe>(modal)
            .map_err(|e| HostError::InvalidModal(e.to_string()))?
            .into();
        serenity::CreateInteractionResponse::Modal(modal)
            .execute(&self.http, interaction_id.into(), token)
            .await?;
        Ok(())
    }
}

/// An error from a [`HostIo`] operation.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// The Discord API rejected the request.
    #[error("discord api error: {0}")]
    Serenity(#[from] serenity::Error),
    /// The modal spec could not be parsed into a Discord modal.
    #[error("invalid modal spec: {0}")]
    InvalidModal(String),
}

/// Builds the wire payload for a [`HostIo::send_message`] call: the prose in
/// a text display inside a Components V2 envelope, with the explicit `tts`
/// and `enforce_nonce` fields the raw HTTP route needs.
fn send_message_payload(content: &str) -> Value {
    pwr_poise_components::view_data_v2([pwr_poise_components::text_display(content)])
}

/// The seam between plugin `host.kv.*` ops and key-value storage. The real
/// implementation wraps the Postgres `plugin_kv` table; tests use the mockall
/// mock generated from this trait, so no plugin test touches a database.
#[automock]
#[async_trait]
pub trait KvStore: Send + Sync {
    /// Returns the value for a key in a namespace, or `None` when unset.
    async fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, KvError>;

    /// Upserts the value for a key in a namespace.
    async fn set(&self, namespace: &str, key: &str, value: &str) -> Result<(), KvError>;

    /// Deletes a key in a namespace. No-op when the key is absent.
    async fn delete(&self, namespace: &str, key: &str) -> Result<(), KvError>;
}

/// An error from a [`KvStore`] operation.
#[derive(Debug, thiserror::Error)]
pub enum KvError {
    /// The underlying database rejected the operation.
    #[error(transparent)]
    Database(#[from] crate::repo::error::DatabaseError),
}

/// The real [`KvStore`], backed by the Postgres `plugin_kv` table. Production
/// wiring of this store into [`HostServices`] happens in #113; tests build it
/// through the seam directly.
pub struct PgKvStore {
    repo: Box<dyn crate::repo::traits::PluginKvRepository + Send + Sync>,
}

impl PgKvStore {
    /// Wraps a plugin key-value repository handle.
    pub fn new(repo: Box<dyn crate::repo::traits::PluginKvRepository + Send + Sync>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl KvStore for PgKvStore {
    async fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, KvError> {
        Ok(self.repo.get(namespace, key).await?)
    }

    async fn set(&self, namespace: &str, key: &str, value: &str) -> Result<(), KvError> {
        self.repo.set(namespace, key, value).await?;
        Ok(())
    }

    async fn delete(&self, namespace: &str, key: &str) -> Result<(), KvError> {
        self.repo.delete(namespace, key).await?;
        Ok(())
    }
}

/// The host configuration subset served to plugins via `host.get_config`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostConfig {
    /// PostgreSQL connection url.
    pub db_url: String,
    /// Directory for bot data (plugin data, caches).
    pub data_path: PathBuf,
    /// How often background tasks poll for changes.
    pub poll_interval: Duration,
}

impl From<&crate::config::Config> for HostConfig {
    fn from(config: &crate::config::Config) -> Self {
        Self {
            db_url: config.db_url.clone(),
            data_path: config.data_path.clone(),
            poll_interval: config.poll_interval,
        }
    }
}

/// An error from a [`StatsSource`] operation.
#[derive(Debug, thiserror::Error)]
pub enum StatsError {
    /// The gateway cache is not attached yet: the host is still starting.
    #[error("gateway cache is not attached yet")]
    Unavailable,
    /// The Discord API rejected the latency probe.
    #[error("discord api error: {0}")]
    Http(#[from] serenity::Error),
}

/// The seam between plugin `host.stats` op and live bot statistics. The real
/// implementation reads the gateway cache and probes the REST API for
/// latency; tests use the mockall mock generated from this trait, so no
/// plugin test touches a live cache or gateway.
#[automock]
#[async_trait]
pub trait StatsSource: Send + Sync {
    /// Gathers the live bot stats snapshot served by `host.stats`.
    async fn stats(&self) -> Result<HostStats, StatsError>;
}

/// The real [`StatsSource`], backed by the bot's gateway cache and HTTP
/// client. Created at host construction with the facts known then (uptime
/// start, version, command count); the real gateway cache exists only inside
/// the `Client` after startup, so it is attached once via
/// [`SerenityStatsSource::attach_cache`] — until then every gather answers
/// [`StatsError::Unavailable`].
pub struct SerenityStatsSource {
    cache: OnceLock<Arc<serenity::Cache>>,
    http: Arc<serenity::Http>,
    start_time: Instant,
    version: String,
    command_count: usize,
}

impl SerenityStatsSource {
    /// Wraps the bot's HTTP client and the stats facts known before the
    /// gateway cache exists.
    pub fn new(
        http: Arc<serenity::Http>,
        start_time: Instant,
        version: String,
        command_count: usize,
    ) -> Self {
        Self {
            cache: OnceLock::new(),
            http,
            start_time,
            version,
            command_count,
        }
    }

    /// Attaches the real gateway cache once the Discord client has been
    /// built. Best-effort: a second attach is ignored (the handle may only be
    /// filled once).
    pub fn attach_cache(&self, cache: Arc<serenity::Cache>) {
        let _ = self.cache.set(cache);
    }
}

#[async_trait]
impl StatsSource for SerenityStatsSource {
    async fn stats(&self) -> Result<HostStats, StatsError> {
        let Some(cache) = self.cache.get() else {
            return Err(StatsError::Unavailable);
        };
        let guilds = cache.guilds();
        let guild_count = guilds.len() as u64;
        let user_count: u64 = guilds
            .iter()
            .filter_map(|guild_id| {
                cache
                    .guild(*guild_id)
                    .map(|guild| guild.member_count.get() as u64)
            })
            .sum();

        // Make a request to Discord server to get latency, like /about does.
        let latency_start = Instant::now();
        let _ = self.http.get_current_user().await?;
        let latency_ms = latency_start.elapsed().as_millis() as u64;

        Ok(HostStats {
            version: self.version.clone(),
            uptime_secs: self.start_time.elapsed().as_secs(),
            guild_count,
            user_count,
            latency_ms,
            command_count: self.command_count as u64,
            memory_mb: crate::bot::utils::process_memory_mb(),
        })
    }
}

/// Interior-mutable slot for the live [`StatsSource`], held inside the
/// [`HostServices`] arc plugins already carry. The source is attached once
/// (with the real gateway cache, at/after client start); before that, every
/// `host.stats` call answers [`StatsError::Unavailable`].
#[derive(Default)]
pub struct StatsHandle {
    source: OnceLock<Arc<dyn StatsSource>>,
}

impl StatsHandle {
    /// Attaches the live stats source. Best-effort: a second attach is
    /// ignored.
    pub fn attach(&self, source: Arc<dyn StatsSource>) {
        let _ = self.source.set(source);
    }

    /// Serves the `host.stats` op: the attached source's snapshot, or
    /// [`StatsError::Unavailable`] before attachment.
    pub async fn stats(&self) -> Result<HostStats, StatsError> {
        let source = self.source.get().ok_or(StatsError::Unavailable)?;
        source.stats().await
    }
}

/// An error from a [`FeedSettingsSource`] operation.
#[derive(Debug, thiserror::Error)]
pub enum FeedSettingsError {
    /// The feed service failed (database or feed-layer error).
    #[error(transparent)]
    Service(#[from] ServiceError),
}

/// The seam between the `host.feed.*` ops and the feed subscription service,
/// mirroring `FeedSubscriptionProvider::get_server_settings` /
/// `update_server_settings` one-to-one (ADR-0010: ops are shaped by
/// services). The real implementation wraps the service the host already
/// holds; tests use the mockall mock generated from this trait.
#[automock]
#[async_trait]
pub trait FeedSettingsSource: Send + Sync {
    /// Reads a guild's whole settings snapshot, as
    /// `FeedSubscriptionProvider::get_server_settings` does.
    async fn get_settings(&self, guild_id: u64) -> Result<ServerSettings, FeedSettingsError>;

    /// Writes a guild's whole settings snapshot, as
    /// `FeedSubscriptionProvider::update_server_settings` does.
    async fn update_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), FeedSettingsError>;
}

/// The real [`FeedSettingsSource`]: a thin adapter over the feed subscription
/// service the host holds at construction. No post-start attachment — the
/// service Arc exists at `Bot::new`, so the field is wired once.
pub struct ServiceFeedSettingsSource {
    service: Arc<dyn FeedSubscriptionProvider>,
}

impl ServiceFeedSettingsSource {
    /// Wraps the host's feed subscription service.
    pub fn new(service: Arc<dyn FeedSubscriptionProvider>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl FeedSettingsSource for ServiceFeedSettingsSource {
    async fn get_settings(&self, guild_id: u64) -> Result<ServerSettings, FeedSettingsError> {
        Ok(self.service.get_server_settings(guild_id).await?)
    }

    async fn update_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), FeedSettingsError> {
        Ok(self
            .service
            .update_server_settings(guild_id, settings)
            .await?)
    }
}

/// An error from a [`VoiceSettingsSource`] operation.
///
/// The voice service pair returns `anyhow::Result` rather than
/// `Result<_, ServiceError>` (the one asymmetry with the feed seam), so this
/// carries the service's opaque error: `Display` renders the anyhow chain's
/// top message, which is what crosses the wire as the [`WireError`] msg.
#[derive(Debug, thiserror::Error)]
pub enum VoiceSettingsError {
    /// The voice service failed (database or voice-layer error).
    #[error(transparent)]
    Service(#[from] anyhow::Error),
}

/// The seam between the `host.voice.*` ops and the voice tracking service,
/// mirroring `VoiceTracker::get_server_settings` /
/// `update_server_settings` one-to-one (ADR-0010: ops are shaped by
/// services). The real implementation wraps the service the host already
/// holds; tests use the mockall mock generated from this trait.
#[automock]
#[async_trait]
pub trait VoiceSettingsSource: Send + Sync {
    /// Reads a guild's whole settings snapshot, as
    /// `VoiceTracker::get_server_settings` does.
    async fn get_settings(&self, guild_id: u64) -> Result<ServerSettings, VoiceSettingsError>;

    /// Writes a guild's whole settings snapshot, as
    /// `VoiceTracker::update_server_settings` does.
    async fn update_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), VoiceSettingsError>;
}

/// The real [`VoiceSettingsSource`]: a thin adapter over the voice tracking
/// service the host holds at construction. No post-start attachment — the
/// service Arc exists at `Bot::new`, so the field is wired once.
pub struct ServiceVoiceSettingsSource {
    service: Arc<dyn VoiceTracker>,
}

impl ServiceVoiceSettingsSource {
    /// Wraps the host's voice tracking service.
    pub fn new(service: Arc<dyn VoiceTracker>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl VoiceSettingsSource for ServiceVoiceSettingsSource {
    async fn get_settings(&self, guild_id: u64) -> Result<ServerSettings, VoiceSettingsError> {
        Ok(self.service.get_server_settings(guild_id).await?)
    }

    async fn update_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), VoiceSettingsError> {
        Ok(self
            .service
            .update_server_settings(guild_id, settings)
            .await?)
    }
}

/// An error from a [`WelcomeSettingsSource`] operation.
#[derive(Debug, thiserror::Error)]
pub enum WelcomeSettingsError {
    /// The welcome settings failed (database or feed-layer error). Welcome
    /// settings ride the same [`FeedSubscriptionProvider`] service the feed
    /// panel persists through, so this mirrors [`FeedSettingsError`].
    #[error(transparent)]
    Service(#[from] ServiceError),
}

/// The seam between the `host.welcome.*` ops and the settings service,
/// mirroring the monolith welcome panel's persistence through
/// `FeedSubscriptionProvider::get_server_settings` / `update_server_settings`
/// one-to-one (ADR-0010: ops are shaped by services). The real
/// implementation wraps the service the host already holds; tests use the
/// mockall mock generated from this trait.
#[automock]
#[async_trait]
pub trait WelcomeSettingsSource: Send + Sync {
    /// Reads a guild's whole settings snapshot, as
    /// `FeedSubscriptionProvider::get_server_settings` does.
    async fn get_settings(&self, guild_id: u64) -> Result<ServerSettings, WelcomeSettingsError>;

    /// Writes a guild's whole settings snapshot, as
    /// `FeedSubscriptionProvider::update_server_settings` does.
    async fn update_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), WelcomeSettingsError>;
}

/// The real [`WelcomeSettingsSource`]: a thin adapter over the feed
/// subscription service the host holds at construction — the same service the
/// monolith's welcome `EffectHandler` persists through.
pub struct ServiceWelcomeSettingsSource {
    service: Arc<dyn FeedSubscriptionProvider>,
}

impl ServiceWelcomeSettingsSource {
    /// Wraps the host's feed subscription service.
    pub fn new(service: Arc<dyn FeedSubscriptionProvider>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl WelcomeSettingsSource for ServiceWelcomeSettingsSource {
    async fn get_settings(&self, guild_id: u64) -> Result<ServerSettings, WelcomeSettingsError> {
        Ok(self.service.get_server_settings(guild_id).await?)
    }

    async fn update_settings(
        &self,
        guild_id: u64,
        settings: ServerSettings,
    ) -> Result<(), WelcomeSettingsError> {
        Ok(self
            .service
            .update_server_settings(guild_id, settings)
            .await?)
    }
}

/// What the host can serve a plugin: the Discord I/O seam, the config subset,
/// the key-value store, the interaction engine used to open `host.open_view`
/// sessions, and the live-stats handle. All are optional so a plugin can be
/// spawned without any of them and still answer `UnknownOp`/`Unavailable`
/// cleanly. The stats handle is always present (it carries no dependencies at
/// construction), but its source is attached only once the host's gateway
/// cache exists.
#[derive(Clone, Default)]
pub struct HostServices {
    /// Discord I/O seam, absent when the host is not wired to Discord.
    pub io: Option<Arc<dyn HostIo>>,
    /// Host config subset, absent when the host has no config to serve.
    pub config: Option<HostConfig>,
    /// Key-value store, absent when the host has no storage wired in.
    pub kv: Option<Arc<dyn KvStore>>,
    /// Interaction engine, absent when the host cannot open target sessions.
    pub engine: Option<Arc<InteractionEngine<RunningPlugin>>>,
    /// Live bot stats for `host.stats`; serves `Unavailable` until the real
    /// gateway cache is attached after client start.
    pub stats: Arc<StatsHandle>,
    /// Feed settings for the `host.feed.*` ops, mirroring the feed
    /// subscription service; absent when the host holds no feed service.
    pub feeds: Option<Arc<dyn FeedSettingsSource>>,
    /// Voice settings for the `host.voice.*` ops, mirroring the voice
    /// tracking service; absent when the host holds no voice service.
    pub voice: Option<Arc<dyn VoiceSettingsSource>>,
    /// Welcome settings for the `host.welcome.*` ops, mirroring the monolith
    /// welcome panel's persistence (the feed subscription service); absent
    /// when the host holds no welcome service.
    pub welcome: Option<Arc<dyn WelcomeSettingsSource>>,
    /// Fills the attachment slots a plugin envelope declares at transport
    /// (ADR-0012); absent when the host holds no preview renderer.
    pub previews: Option<Arc<crate::plugin::preview::PreviewResolver>>,
}

/// Serves one plugin→host [`Msg::Call`], answering with the correlation-id
/// matched `resp`. Every failure crosses the wire as a first-class
/// [`WireError`]; nothing panics on unknown ops or missing services. The
/// plugin manager resolves targets for `host.open_view` and receives
/// `host.open_modal` route bindings; it is optional so plain spawns without a
/// manager still answer `HostUnavailable`. `caller` is the manager-registered
/// name of the plugin session whose reader received the call — the
/// host-stamped owner for ops that bind state to the calling session, so a
/// plugin cannot bind a route under another plugin's name.
pub async fn handle_host_call(
    id: u64,
    caller: &str,
    op: &str,
    args: Option<&Value>,
    host: Option<&HostServices>,
    manager: Option<&PluginManager>,
) -> Msg {
    let Some(cap) = HostCap::parse(op) else {
        return resp_err(
            id,
            "UnknownOp",
            format!("unknown op `{op}` (non-host.* and unknown host.* ops are not served)"),
        );
    };
    match cap {
        HostCap::KvGet | HostCap::KvSet | HostCap::KvDelete => {
            let Some(kv) = host.and_then(|host| host.kv.clone()) else {
                return resp_err(id, "KvUnavailable", "kv store is not configured");
            };
            match kv_call(cap, args, &*kv).await {
                Ok(data) => Msg::resp_ok(id, data),
                Err(wire) => Msg::resp_err(id, wire),
            }
        }
        HostCap::GetConfig => {
            let Some(config) = host.and_then(|host| host.config.as_ref()) else {
                return resp_err(id, "ConfigUnavailable", "host config is not configured");
            };
            Msg::resp_ok(
                id,
                Some(json!({
                    "db_url": config.db_url,
                    "data_path": config.data_path,
                    "poll_interval": config.poll_interval.as_secs(),
                })),
            )
        }
        HostCap::Defer | HostCap::Acknowledge | HostCap::SendMessage | HostCap::EditMessage => {
            let Some(io) = host.and_then(|host| host.io.clone()) else {
                return resp_err(id, "HostUnavailable", "host io is not configured");
            };
            match io_call(cap, args, &*io).await {
                Ok(data) => Msg::resp_ok(id, data),
                Err(wire) => Msg::resp_err(id, wire),
            }
        }
        HostCap::OpenView => {
            let Some(io) = host.and_then(|host| host.io.clone()) else {
                return resp_err(id, "HostUnavailable", "host io is not configured");
            };
            let Some(engine) = host.and_then(|host| host.engine.clone()) else {
                return resp_err(
                    id,
                    "HostUnavailable",
                    "host interaction engine is not configured",
                );
            };
            let Some(manager) = manager else {
                return resp_err(
                    id,
                    "HostUnavailable",
                    "host plugin manager is not configured",
                );
            };
            match open_view_call(
                args,
                &*io,
                &engine,
                manager,
                host.and_then(|host| host.previews.as_deref()),
            )
            .await
            {
                Ok(data) => Msg::resp_ok(id, data),
                Err(wire) => Msg::resp_err(id, wire),
            }
        }
        HostCap::OpenModal => {
            let Some(io) = host.and_then(|host| host.io.clone()) else {
                return resp_err(id, "HostUnavailable", "host io is not configured");
            };
            let Some(manager) = manager else {
                return resp_err(
                    id,
                    "HostUnavailable",
                    "host plugin manager is not configured",
                );
            };
            match open_modal_call(args, &*io, manager, caller).await {
                Ok(data) => Msg::resp_ok(id, data),
                Err(wire) => Msg::resp_err(id, wire),
            }
        }
        HostCap::ListPlugins => {
            let Some(manager) = manager else {
                return resp_err(
                    id,
                    "HostUnavailable",
                    "host plugin manager is not configured",
                );
            };
            let names = manager.running_names().await;
            Msg::resp_ok(id, Some(json!({ "plugins": names })))
        }
        HostCap::Stats => {
            let Some(stats) = host.map(|host| host.stats.clone()) else {
                return resp_err(id, "HostUnavailable", "host stats are not configured");
            };
            match stats.stats().await {
                Ok(data) => match serde_json::to_value(&data) {
                    Ok(value) => Msg::resp_ok(id, Some(value)),
                    Err(e) => Msg::resp_err(
                        id,
                        WireError {
                            kind: "StatsError".into(),
                            msg: e.to_string(),
                        },
                    ),
                },
                Err(e) => Msg::resp_err(id, stats_err(e)),
            }
        }
        HostCap::FeedGetSettings => {
            let Some(feeds) = host.and_then(|host| host.feeds.clone()) else {
                return resp_err(id, "HostUnavailable", "feed settings are not configured");
            };
            match feed_call(cap, args, &*feeds).await {
                Ok(data) => Msg::resp_ok(id, data),
                Err(wire) => Msg::resp_err(id, wire),
            }
        }
        HostCap::FeedUpdateSettings => {
            let Some(feeds) = host.and_then(|host| host.feeds.clone()) else {
                return resp_err(id, "HostUnavailable", "feed settings are not configured");
            };
            match feed_call(cap, args, &*feeds).await {
                Ok(data) => Msg::resp_ok(id, data),
                Err(wire) => Msg::resp_err(id, wire),
            }
        }
        HostCap::VoiceGetSettings => {
            let Some(voice) = host.and_then(|host| host.voice.clone()) else {
                return resp_err(id, "HostUnavailable", "voice settings are not configured");
            };
            match voice_call(cap, args, &*voice).await {
                Ok(data) => Msg::resp_ok(id, data),
                Err(wire) => Msg::resp_err(id, wire),
            }
        }
        HostCap::VoiceUpdateSettings => {
            let Some(voice) = host.and_then(|host| host.voice.clone()) else {
                return resp_err(id, "HostUnavailable", "voice settings are not configured");
            };
            match voice_call(cap, args, &*voice).await {
                Ok(data) => Msg::resp_ok(id, data),
                Err(wire) => Msg::resp_err(id, wire),
            }
        }
        HostCap::WelcomeGetSettings => {
            let Some(welcome) = host.and_then(|host| host.welcome.clone()) else {
                return resp_err(id, "HostUnavailable", "welcome settings are not configured");
            };
            match welcome_call(cap, args, &*welcome).await {
                Ok(data) => Msg::resp_ok(id, data),
                Err(wire) => Msg::resp_err(id, wire),
            }
        }
        HostCap::WelcomeUpdateSettings => {
            let Some(welcome) = host.and_then(|host| host.welcome.clone()) else {
                return resp_err(id, "HostUnavailable", "welcome settings are not configured");
            };
            match welcome_call(cap, args, &*welcome).await {
                Ok(data) => Msg::resp_ok(id, data),
                Err(wire) => Msg::resp_err(id, wire),
            }
        }
    }
}

/// Runs one I/O-backed host op against the seam and turns the outcome into a
/// wire value: `Ok(data)` for a successful `resp_ok`, `Err(wire)` for a
/// failed `resp_err` (`InvalidArgs` or `HostIoError`). A cap outside the I/O
/// set is a dispatch bug, so it answers `UnknownOp` rather than panicking the
/// dispatch task.
async fn io_call(
    cap: HostCap,
    args: Option<&Value>,
    io: &dyn HostIo,
) -> Result<Option<Value>, WireError> {
    match cap {
        HostCap::Defer => {
            let (interaction_id, token) = parse_id_token(args)?;
            io.defer(interaction_id, &token)
                .await
                .map_err(host_io_err)?;
            Ok(None)
        }
        HostCap::Acknowledge => {
            let (interaction_id, token) = parse_id_token(args)?;
            io.acknowledge(interaction_id, &token)
                .await
                .map_err(host_io_err)?;
            Ok(None)
        }
        HostCap::SendMessage => {
            let (channel_id, content) = parse_send_message(args)?;
            io.send_message(channel_id, &content)
                .await
                .map_err(host_io_err)
        }
        HostCap::EditMessage => {
            let (channel_id, message_id, data) = parse_edit_message(args)?;
            reject_content_on_edit(&data).map_err(WireError::from)?;
            io.edit_message(
                channel_id,
                message_id,
                edit_body_for_transport(&data),
                Vec::new(),
            )
            .await
            .map_err(host_io_err)
        }
        other => Err(WireError {
            kind: "UnknownOp".into(),
            msg: format!("op `{}` is not an I/O op", other.as_str()),
        }),
    }
}

/// Runs the `host.open_view` op end to end: resolves the target plugin from
/// the manager, renders the target's panel into an interaction-engine
/// session, and edits the message body. When the call carries a `message_id`
/// (the in-place path) that message is the edit target and the session
/// replaces whatever was open on it; otherwise a placeholder is posted
/// through the io seam and the session opens on the produced id. Returns the
/// message id the session opened on so the caller can route interactions.
async fn open_view_call(
    args: Option<&Value>,
    io: &dyn HostIo,
    engine: &InteractionEngine<RunningPlugin>,
    manager: &PluginManager,
    previews: Option<&crate::plugin::preview::PreviewResolver>,
) -> Result<Option<Value>, WireError> {
    let (channel_id, plugin_name, command, call_args, message_id) = parse_open_view(args)?;
    let guild_id = call_args.get("guild_id").and_then(id_as_u64);
    let Some(target) = manager.get(&plugin_name).await else {
        return Err(WireError {
            kind: "PluginNotFound".into(),
            msg: format!("target plugin `{plugin_name}` is not running"),
        });
    };
    let spec = match engine.invoke(target.clone(), &command, call_args).await {
        Ok(spec) => spec,
        Err(e) => return Err(open_view_err(e)),
    };
    if let Err(error) = validate_view_data(&spec.data) {
        return Err(error.into());
    }
    let message_id = match message_id {
        Some(message_id) => message_id,
        None => {
            let placeholder = io
                .send_message(channel_id, "Loading…")
                .await
                .map_err(host_io_err)?;
            placeholder
                .as_ref()
                .and_then(|data| data.get("message_id"))
                .and_then(Value::as_u64)
                .ok_or_else(|| WireError {
                    kind: "HostIoError".into(),
                    msg: "send_message resp carried no message id".into(),
                })?
        }
    };
    engine
        .register(
            serenity::MessageId::new(message_id),
            target,
            &command,
            spec.clone(),
        )
        .await;
    let (body, attachments) = match previews {
        Some(previews) => {
            previews
                .resolve(edit_body_for_transport(&spec.data), guild_id)
                .await
        }
        None => (edit_body_for_transport(&spec.data), Vec::new()),
    };
    if let Err(e) = io
        .edit_message(channel_id, message_id, body, attachments)
        .await
    {
        // The session is live on the placeholder, but the placeholder never
        // resolved to the final payload; abandon the session so it does not
        // leak in the engine, mirroring the router's dead-session handling.
        if let Err(e) = engine.abandon(serenity::MessageId::new(message_id)).await {
            warn!("failed to abandon open_view session for message {message_id}: {e}");
        }
        return Err(host_io_err(e));
    }
    Ok(Some(json!({ "message_id": message_id })))
}

/// Maps an interaction-engine failure during [`open_view_call`] to a wire
/// error: a plugin rejection forwards the target's own kind, engine failures
/// get a host-side kind.
fn open_view_err(err: InteractionError) -> WireError {
    match err {
        InteractionError::PluginRejected { kind, msg } => WireError { kind, msg },
        InteractionError::Plugin(e) => WireError {
            kind: "PluginOpError".into(),
            msg: e.to_string(),
        },
        InteractionError::UnexpectedReply { detail } => WireError {
            kind: "UnexpectedReply".into(),
            msg: detail,
        },
        InteractionError::InvalidView { kind, msg } => WireError { kind, msg },
        InteractionError::NoSession { .. } => WireError {
            kind: "NoSession".into(),
            msg: err.to_string(),
        },
    }
}

/// Runs the `host.open_modal` op end to end: validates the modal spec
/// (parse-on-clone-discard, the [`validate_view_data`] precedent), opens it
/// as the response to the triggering interaction — the open IS the response
/// (ADR-0007), so an already-acked interaction fails — and binds the
/// author's submission route to the calling plugin session (`caller`, the
/// host-stamped owner). The bind happens only after Discord accepted the
/// open, so a failed open leaves no route behind.
async fn open_modal_call(
    args: Option<&Value>,
    io: &dyn HostIo,
    manager: &PluginManager,
    caller: &str,
) -> Result<Option<Value>, WireError> {
    let (author_id, interaction_id, token, modal) = parse_open_modal(args)?;
    serde_json::from_value::<CreateModalDe>(modal.clone())
        .map_err(|e| invalid_args(&format!("`modal` is not a valid modal spec: {e}")))?;
    // `CreateModalDe` requires `custom_id: Cow<str>`, so a spec that just
    // validated always carries it as a string here.
    let custom_id = modal
        .get("custom_id")
        .and_then(Value::as_str)
        .expect("validated `CreateModalDe` always carries a string `custom_id`")
        .to_string();
    io.open_modal(interaction_id, &token, modal)
        .await
        .map_err(host_io_err)?;
    manager.bind_modal(author_id, caller, &custom_id).await;
    Ok(None)
}

/// Parses `host.open_modal` args: the author of the triggering interaction
/// (the submission's routing key), the interaction id and token the modal
/// opens as a response to, and the modal spec (`custom_id`, `title`,
/// `components`). Ids accept a number or serenity's string form.
fn parse_open_modal(args: Option<&Value>) -> Result<(u64, u64, String, Value), WireError> {
    let obj = args.and_then(Value::as_object).ok_or_else(|| {
        invalid_args("expected args object with `author_id`, `interaction_id`, `token`, `modal`")
    })?;
    let author_id = obj
        .get("author_id")
        .and_then(id_as_u64)
        .ok_or_else(|| invalid_args("missing `author_id` (u64)"))?;
    let interaction_id = obj
        .get("interaction_id")
        .and_then(id_as_u64)
        .ok_or_else(|| invalid_args("missing `interaction_id` (u64)"))?;
    let token = obj
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_args("missing `token` (string)"))?
        .to_string();
    let modal = obj
        .get("modal")
        .cloned()
        .ok_or_else(|| invalid_args("missing `modal` (object)"))?;
    Ok((author_id, interaction_id, token, modal))
}

/// Runs one KV-backed host op against the seam and turns the outcome into a
/// wire value: `Ok(data)` for a successful `resp_ok`, `Err(wire)` for a
/// failed `resp_err` (`InvalidArgs` or `KvStoreError`). An absent key is not
/// an error: `host.kv.get` answers `{"value": null}` so the plugin can apply
/// its default. A cap outside the KV set is a dispatch bug, so it answers
/// `UnknownOp` rather than panicking the dispatch task.
async fn kv_call(
    cap: HostCap,
    args: Option<&Value>,
    kv: &dyn KvStore,
) -> Result<Option<Value>, WireError> {
    match cap {
        HostCap::KvGet => {
            let (namespace, key) = parse_kv_namespace_key(args)?;
            let value = kv.get(&namespace, &key).await.map_err(kv_err)?;
            Ok(Some(json!({ "value": value })))
        }
        HostCap::KvSet => {
            let (namespace, key, value) = parse_kv_set(args)?;
            kv.set(&namespace, &key, &value).await.map_err(kv_err)?;
            Ok(None)
        }
        HostCap::KvDelete => {
            let (namespace, key) = parse_kv_namespace_key(args)?;
            kv.delete(&namespace, &key).await.map_err(kv_err)?;
            Ok(None)
        }
        other => Err(WireError {
            kind: "UnknownOp".into(),
            msg: format!("op `{}` is not a KV op", other.as_str()),
        }),
    }
}

/// Parses the `namespace` and `key` fields shared by all `host.kv.*` ops.
/// Values are `String` only: numbers, booleans, and objects are rejected with
/// `InvalidArgs`.
fn kv_fields(args: Option<&Value>) -> Result<(String, String), WireError> {
    let obj = args.and_then(Value::as_object).ok_or_else(|| {
        invalid_args("expected args object with `namespace` (string) and `key` (string)")
    })?;
    let namespace = obj
        .get("namespace")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_args("missing `namespace` (string)"))?
        .to_string();
    let key = obj
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_args("missing `key` (string)"))?
        .to_string();
    Ok((namespace, key))
}

/// Parses `host.kv.get` / `host.kv.delete` args: a namespace and key. Values
/// are `String` only: numbers, booleans, and objects are rejected with
/// `InvalidArgs`.
fn parse_kv_namespace_key(args: Option<&Value>) -> Result<(String, String), WireError> {
    kv_fields(args)
}

/// Parses `host.kv.set` args: a namespace, key, and value. Values are
/// `String` only: numbers, booleans, and objects are rejected with
/// `InvalidArgs`.
fn parse_kv_set(args: Option<&Value>) -> Result<(String, String, String), WireError> {
    let (namespace, key) = kv_fields(args)?;
    let obj = args.and_then(Value::as_object).ok_or_else(|| {
        invalid_args("expected args object with `namespace`, `key`, and `value` (strings)")
    })?;
    let value = obj
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_args("missing `value` (string)"))?
        .to_string();
    Ok((namespace, key, value))
}

/// Parses `host.defer` / `host.acknowledge` args: an interaction id and token.
fn parse_id_token(args: Option<&Value>) -> Result<(u64, String), WireError> {
    let obj = args.and_then(Value::as_object).ok_or_else(|| {
        invalid_args("expected args object with `interaction_id` (u64) and `token` (string)")
    })?;
    let interaction_id = obj
        .get("interaction_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_args("missing `interaction_id` (u64)"))?;
    let token = obj
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_args("missing `token` (string)"))?
        .to_string();
    Ok((interaction_id, token))
}

/// Parses `host.send_message` args: a channel id and the prose to send. The
/// prose renders as a text display inside a Components V2 envelope; unknown
/// argument keys (e.g. a legacy plugin's `data` object) have no effect and
/// are logged at debug level.
fn parse_send_message(args: Option<&Value>) -> Result<(u64, String), WireError> {
    let obj = args.and_then(Value::as_object).ok_or_else(|| {
        invalid_args("expected args object with `channel_id` (u64) and `content` (string)")
    })?;
    let ignored: Vec<&str> = obj
        .keys()
        .map(String::as_str)
        .filter(|key| !matches!(*key, "channel_id" | "content"))
        .collect();
    if !ignored.is_empty() {
        debug!("host.send_message ignored argument keys: {ignored:?}");
    }
    let channel_id = obj
        .get("channel_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_args("missing `channel_id` (u64)"))?;
    let content = obj
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_args("missing `content` (string)"))?
        .to_string();
    Ok((channel_id, content))
}

/// Parses `host.edit_message` args: a channel id, message id, and the edit
/// body. Only the fields the body provides are applied — partial shapes are
/// legal; the content gate is the only rule enforced on it.
fn parse_edit_message(args: Option<&Value>) -> Result<(u64, u64, Value), WireError> {
    let obj = args.and_then(Value::as_object).ok_or_else(|| {
        invalid_args("expected args object with `channel_id`, `message_id`, `data`")
    })?;
    let channel_id = obj
        .get("channel_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_args("missing `channel_id` (u64)"))?;
    let message_id = obj
        .get("message_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_args("missing `message_id` (u64)"))?;
    let data = obj
        .get("data")
        .cloned()
        .ok_or_else(|| invalid_args("missing `data` (object)"))?;
    Ok((channel_id, message_id, data))
}

/// Parses `host.open_view` args: the channel to post into, the target plugin
/// name, the target command (defaults to the plugin name), the invoke args
/// forwarded to the target (defaults to `{}`), and the optional source
/// message id to edit in place (absent: a fresh placeholder is posted).
fn parse_open_view(
    args: Option<&Value>,
) -> Result<(u64, String, String, Value, Option<u64>), WireError> {
    let obj = args.and_then(Value::as_object).ok_or_else(|| {
        invalid_args("expected args object with `channel_id` (u64) and `plugin` (string)")
    })?;
    let channel_id = obj
        .get("channel_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_args("missing `channel_id` (u64)"))?;
    let plugin = obj
        .get("plugin")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_args("missing `plugin` (string)"))?
        .to_string();
    let command = match obj.get("command") {
        None => plugin.clone(),
        Some(command) => command
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| invalid_args("`command` must be a string"))?,
    };
    let call_args = match obj.get("args") {
        None | Some(Value::Null) => json!({}),
        Some(args) if args.is_object() => args.clone(),
        Some(_) => return Err(invalid_args("`args` must be an object")),
    };
    let message_id = obj.get("message_id").and_then(id_as_u64);
    Ok((channel_id, plugin, command, call_args, message_id))
}

fn invalid_args(msg: &str) -> WireError {
    WireError {
        kind: "InvalidArgs".into(),
        msg: msg.into(),
    }
}

fn host_io_err(err: HostError) -> WireError {
    WireError {
        kind: "HostIoError".into(),
        msg: err.to_string(),
    }
}

fn kv_err(err: KvError) -> WireError {
    WireError {
        kind: "KvStoreError".into(),
        msg: err.to_string(),
    }
}

/// Maps a [`StatsError`] to a wire error: an unattached source is the same
/// `HostUnavailable` a missing service answers, any other gather failure is a
/// `StatsError`.
fn stats_err(err: StatsError) -> WireError {
    match err {
        StatsError::Unavailable => WireError {
            kind: "HostUnavailable".into(),
            msg: "host stats are not attached yet".into(),
        },
        StatsError::Http(e) => WireError {
            kind: "StatsError".into(),
            msg: e.to_string(),
        },
    }
}

/// Runs one feed-settings op against the seam and turns the outcome into a
/// wire value: `Ok(data)` for a successful `resp_ok` (the settings snapshot
/// for a read, `None` for a write), `Err(wire)` for a failed `resp_err`
/// (`InvalidArgs` or `FeedSettingsError`). A cap outside the feed pair is a
/// dispatch bug, so it answers `UnknownOp` rather than panicking the dispatch
/// task.
async fn feed_call(
    cap: HostCap,
    args: Option<&Value>,
    feeds: &dyn FeedSettingsSource,
) -> Result<Option<Value>, WireError> {
    match cap {
        HostCap::FeedGetSettings => {
            let guild_id = parse_guild_id(args)?;
            let settings = feeds
                .get_settings(guild_id)
                .await
                .map_err(feed_settings_err)?;
            let value = serde_json::to_value(&settings).map_err(|e| WireError {
                kind: "FeedSettingsError".into(),
                msg: e.to_string(),
            })?;
            Ok(Some(value))
        }
        HostCap::FeedUpdateSettings => {
            let (guild_id, settings) = parse_update_settings(args)?;
            feeds
                .update_settings(guild_id, settings)
                .await
                .map_err(feed_settings_err)?;
            Ok(None)
        }
        other => Err(WireError {
            kind: "UnknownOp".into(),
            msg: format!("op `{}` is not a feed-settings op", other.as_str()),
        }),
    }
}

/// Reads a Discord id from a wire value: a number, or the string form
/// serenity's ids serialize to — the same rule the plugins' id parsing uses,
/// so the host accepts exactly what a plugin may legitimately send.
fn id_as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

/// Parses the `guild_id` (u64) shared by the `host.feed.*`, `host.voice.*`,
/// and `host.welcome.*` ops. Accepts a numeric id or serenity's string form. A
/// present id that is neither is a wrong type, not a missing one.
fn parse_guild_id(args: Option<&Value>) -> Result<u64, WireError> {
    let Some(value) = args
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("guild_id"))
    else {
        return Err(invalid_args("missing `guild_id` (u64)"));
    };
    id_as_u64(value).ok_or_else(|| invalid_args("`guild_id` must be a u64 or its string form"))
}

/// Parses the `host.feed.update_settings`, `host.voice.update_settings`, and
/// `host.welcome.update_settings` args: `guild_id` (u64) plus the whole
/// [`ServerSettings`] snapshot under `settings`.
fn parse_update_settings(args: Option<&Value>) -> Result<(u64, ServerSettings), WireError> {
    let guild_id = parse_guild_id(args)?;
    let settings = args
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("settings"))
        .ok_or_else(|| invalid_args("missing `settings` (ServerSettings object)"))?;
    serde_json::from_value(settings.clone())
        .map_err(|e| invalid_args(&format!("`settings` is not a ServerSettings: {e}")))
        .map(|settings| (guild_id, settings))
}

/// Maps a [`FeedSettingsError`] to its wire error.
fn feed_settings_err(err: FeedSettingsError) -> WireError {
    WireError {
        kind: "FeedSettingsError".into(),
        msg: err.to_string(),
    }
}

/// Runs one voice-settings op against the seam and turns the outcome into a
/// wire value: `Ok(data)` for a successful `resp_ok` (the settings snapshot
/// for a read, `None` for a write), `Err(wire)` for a failed `resp_err`
/// (`InvalidArgs` or `VoiceSettingsError`). A cap outside the voice pair is a
/// dispatch bug, so it answers `UnknownOp` rather than panicking the dispatch
/// task.
async fn voice_call(
    cap: HostCap,
    args: Option<&Value>,
    voice: &dyn VoiceSettingsSource,
) -> Result<Option<Value>, WireError> {
    match cap {
        HostCap::VoiceGetSettings => {
            let guild_id = parse_guild_id(args)?;
            let settings = voice
                .get_settings(guild_id)
                .await
                .map_err(voice_settings_err)?;
            let value = serde_json::to_value(&settings).map_err(|e| WireError {
                kind: "VoiceSettingsError".into(),
                msg: e.to_string(),
            })?;
            Ok(Some(value))
        }
        HostCap::VoiceUpdateSettings => {
            let (guild_id, settings) = parse_update_settings(args)?;
            voice
                .update_settings(guild_id, settings)
                .await
                .map_err(voice_settings_err)?;
            Ok(None)
        }
        other => Err(WireError {
            kind: "UnknownOp".into(),
            msg: format!("op `{}` is not a voice-settings op", other.as_str()),
        }),
    }
}

/// Maps a [`VoiceSettingsError`] to its wire error.
fn voice_settings_err(err: VoiceSettingsError) -> WireError {
    WireError {
        kind: "VoiceSettingsError".into(),
        msg: err.to_string(),
    }
}

/// Runs one welcome-settings op against the seam and turns the outcome into a
/// wire value: `Ok(data)` for a successful `resp_ok` (the settings snapshot
/// for a read, `None` for a write), `Err(wire)` for a failed `resp_err`
/// (`InvalidArgs` or `WelcomeSettingsError`). A cap outside the welcome pair
/// is a dispatch bug, so it answers `UnknownOp` rather than panicking the
/// dispatch task.
async fn welcome_call(
    cap: HostCap,
    args: Option<&Value>,
    welcome: &dyn WelcomeSettingsSource,
) -> Result<Option<Value>, WireError> {
    match cap {
        HostCap::WelcomeGetSettings => {
            let guild_id = parse_guild_id(args)?;
            let settings = welcome
                .get_settings(guild_id)
                .await
                .map_err(welcome_settings_err)?;
            let value = serde_json::to_value(&settings).map_err(|e| WireError {
                kind: "WelcomeSettingsError".into(),
                msg: e.to_string(),
            })?;
            Ok(Some(value))
        }
        HostCap::WelcomeUpdateSettings => {
            let (guild_id, settings) = parse_update_settings(args)?;
            welcome
                .update_settings(guild_id, settings)
                .await
                .map_err(welcome_settings_err)?;
            Ok(None)
        }
        other => Err(WireError {
            kind: "UnknownOp".into(),
            msg: format!("op `{}` is not a welcome-settings op", other.as_str()),
        }),
    }
}

/// Maps a [`WelcomeSettingsError`] to its wire error.
fn welcome_settings_err(err: WelcomeSettingsError) -> WireError {
    WireError {
        kind: "WelcomeSettingsError".into(),
        msg: err.to_string(),
    }
}

fn resp_err(id: u64, kind: &str, msg: impl Into<String>) -> Msg {
    Msg::resp_err(
        id,
        WireError {
            kind: kind.into(),
            msg: msg.into(),
        },
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use mockall::predicate::always;
    use mockall::predicate::eq;
    use pwr_ext::prelude::CreateMessageDe;
    use serde_json::json;

    use super::*;
    use crate::plugin::ModalRouteError;
    use crate::plugin::RespawnPolicy;

    fn services(io: Option<Arc<dyn HostIo>>, config: Option<HostConfig>) -> HostServices {
        HostServices {
            io,
            config,
            kv: None,
            engine: None,
            stats: Arc::new(StatsHandle::default()),
            feeds: None,
            voice: None,
            welcome: None,
            previews: None,
        }
    }

    fn kv_services(
        io: Option<Arc<dyn HostIo>>,
        config: Option<HostConfig>,
        kv: Option<Arc<dyn KvStore>>,
    ) -> HostServices {
        HostServices {
            io,
            config,
            kv,
            engine: None,
            stats: Arc::new(StatsHandle::default()),
            feeds: None,
            voice: None,
            welcome: None,
            previews: None,
        }
    }

    /// Host services with an interaction engine wired in, for `open_view`
    /// tests that need the engine service present.
    fn view_services(io: Option<Arc<dyn HostIo>>, config: Option<HostConfig>) -> HostServices {
        HostServices {
            io,
            config,
            kv: None,
            engine: Some(Arc::new(InteractionEngine::new())),
            stats: Arc::new(StatsHandle::default()),
            feeds: None,
            voice: None,
            welcome: None,
            previews: None,
        }
    }

    fn sample_config() -> HostConfig {
        HostConfig {
            db_url: "postgres://host".into(),
            data_path: PathBuf::from("/data"),
            poll_interval: Duration::from_secs(30),
        }
    }

    fn assert_ok(resp: Msg, expected_id: u64) -> Option<Value> {
        match resp {
            Msg::Resp {
                id,
                ok: true,
                data,
                error: None,
            } => {
                assert_eq!(
                    id, expected_id,
                    "resp must echo the plugin's correlation id"
                );
                data
            }
            other => panic!("expected ok resp for id {expected_id}, got {other:?}"),
        }
    }

    fn assert_err(resp: Msg, expected_id: u64, kind: &str) -> String {
        match resp {
            Msg::Resp {
                id,
                ok: false,
                error: Some(err),
                ..
            } => {
                assert_eq!(
                    id, expected_id,
                    "resp must echo the plugin's correlation id"
                );
                assert_eq!(err.kind, kind, "error kind");
                err.msg
            }
            other => panic!("expected err resp for id {expected_id}, got {other:?}"),
        }
    }

    // ── defer ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn defer_routes_through_the_seam() {
        let mut mock = MockHostIo::new();
        mock.expect_defer()
            .with(eq(42_u64), eq("token-1"))
            .times(1)
            .returning(|_, _| Ok(()));
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.defer",
            Some(&json!({ "interaction_id": 42, "token": "token-1" })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), None);
    }

    #[tokio::test]
    async fn defer_missing_token_is_invalid_args() {
        let mock = MockHostIo::new();
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.defer",
            Some(&json!({ "interaction_id": 42 })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn defer_without_services_is_host_unavailable() {
        let resp = handle_host_call(7, "hello", "host.defer", None, None, None).await;
        assert_err(resp, 7, "HostUnavailable");
    }

    // ── acknowledge ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn acknowledge_routes_through_the_seam() {
        let mut mock = MockHostIo::new();
        mock.expect_acknowledge()
            .with(eq(42_u64), eq("token-1"))
            .times(1)
            .returning(|_, _| Ok(()));
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.acknowledge",
            Some(&json!({ "interaction_id": 42, "token": "token-1" })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), None);
    }

    // ── send_message ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn send_message_routes_through_the_seam() {
        let mut mock = MockHostIo::new();
        mock.expect_send_message()
            .with(eq(99_u64), eq("hello world"))
            .times(1)
            .returning(|_, _| Ok(Some(json!({ "message_id": 1234 }))));
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.send_message",
            Some(&json!({
                "channel_id": 99,
                "content": "hello world",
                "data": { "flags": 0 },
            })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), Some(json!({ "message_id": 1234 })));
    }

    #[tokio::test]
    async fn send_message_renders_the_prose_as_a_v2_text_display() {
        let payload = send_message_payload("Loading…");

        assert_eq!(
            payload["components"][0],
            json!({ "content": "Loading…", "type": 10 })
        );
        assert_eq!(
            payload["flags"],
            json!(pwr_poise_components::IS_COMPONENTS_V2)
        );
        assert!(payload.get("content").is_none());
        serde_json::from_value::<CreateMessageDe>(payload).unwrap();
    }

    #[tokio::test]
    async fn send_message_missing_content_is_invalid_args() {
        let mock = MockHostIo::new();
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.send_message",
            Some(&json!({ "channel_id": 99 })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn send_message_non_object_data_is_ignored() {
        let mut mock = MockHostIo::new();
        mock.expect_send_message()
            .with(eq(99_u64), eq("x"))
            .times(1)
            .returning(|_, _| Ok(Some(json!({ "message_id": 1 }))));
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.send_message",
            Some(&json!({ "channel_id": 99, "content": "x", "data": "oops" })),
            Some(&host),
            None,
        )
        .await;
        assert_ok(resp, 7);
    }

    // ── edit_message ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn edit_message_routes_through_the_seam() {
        let mut mock = MockHostIo::new();
        mock.expect_edit_message()
            .with(
                eq(99_u64),
                eq(1234_u64),
                eq(json!({ "components": [{ "type": 10, "content": "edited" }] })),
                always(),
            )
            .times(1)
            .returning(|_, _, _, _| Ok(Some(json!({ "message_id": 1234 }))));
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.edit_message",
            Some(&json!({
                "channel_id": 99,
                "message_id": 1234,
                "data": { "components": [{ "type": 10, "content": "edited" }] },
            })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), Some(json!({ "message_id": 1234 })));
    }

    #[tokio::test]
    async fn edit_message_drops_create_only_fields_before_the_seam() {
        // A plugin edit body is built for sending, so it carries create-only
        // fields the channel edit endpoint rejects (error 50080 for
        // `sticker_ids`): the op arm strips them before the transport seam.
        let mut mock = MockHostIo::new();
        mock.expect_edit_message()
            .with(
                eq(99_u64),
                eq(1234_u64),
                eq(json!({
                    "components": [{ "type": 10, "content": "edited" }],
                    "flags": 32768,
                })),
                always(),
            )
            .times(1)
            .returning(|_, _, _, _| Ok(Some(json!({ "message_id": 1234 }))));
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.edit_message",
            Some(&json!({
                "channel_id": 99,
                "message_id": 1234,
                "data": {
                    "components": [{ "type": 10, "content": "edited" }],
                    "flags": 32768,
                    "sticker_ids": [],
                    "tts": false,
                    "enforce_nonce": false,
                    "nonce": "view-42",
                },
            })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), Some(json!({ "message_id": 1234 })));
    }

    #[tokio::test]
    async fn edit_message_missing_data_is_invalid_args() {
        let mock = MockHostIo::new();
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.edit_message",
            Some(&json!({ "channel_id": 99, "message_id": 1234 })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn edit_message_with_content_beside_the_v2_flag_is_invalid_view() {
        let mock = MockHostIo::new();
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.edit_message",
            Some(&json!({
                "channel_id": 99,
                "message_id": 1234,
                "data": {
                    "content": "legacy prose",
                    "flags": 32768,
                },
            })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidView");
    }

    #[tokio::test]
    async fn edit_message_with_content_and_no_flags_is_invalid_view() {
        // An edit cannot unset IS_COMPONENTS_V2, so content beside absent
        // flags still 50035s against an already-V2 message: the arm rejects
        // it even though the V2 flag is not in the payload.
        let mock = MockHostIo::new();
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.edit_message",
            Some(&json!({
                "channel_id": 99,
                "message_id": 1234,
                "data": { "content": "legacy prose" },
            })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidView");
    }

    #[tokio::test]
    async fn edit_message_with_non_string_content_is_invalid_view() {
        let mock = MockHostIo::new();
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.edit_message",
            Some(&json!({
                "channel_id": 99,
                "message_id": 1234,
                "data": { "content": 42, "flags": 32768 },
            })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidView");
    }

    // ── get_config ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn get_config_returns_the_three_fields() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(7, "hello", "host.get_config", None, Some(&host), None).await;
        assert_eq!(
            assert_ok(resp, 7),
            Some(json!({
                "db_url": "postgres://host",
                "data_path": "/data",
                "poll_interval": 30,
            }))
        );
    }

    #[tokio::test]
    async fn get_config_without_config_is_config_unavailable() {
        let host = services(Some(Arc::new(MockHostIo::new())), None);

        let resp = handle_host_call(7, "hello", "host.get_config", None, Some(&host), None).await;
        assert_err(resp, 7, "ConfigUnavailable");
    }

    // ── unknown / unsupported ops ─────────────────────────────────────────────

    #[tokio::test]
    async fn unknown_host_op_is_unknown_op() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(7, "hello", "host.frobnicate", None, Some(&host), None).await;
        let msg = assert_err(resp, 7, "UnknownOp");
        assert!(msg.contains("host.frobnicate"), "msg: {msg}");
    }

    #[tokio::test]
    async fn non_host_op_is_unknown_op() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(7, "hello", "invoke", None, Some(&host), None).await;
        assert_err(resp, 7, "UnknownOp");
    }

    // ── open_view ─────────────────────────────────────────────────────────────
    // The resolution happy path (placeholder → engine session → edit) needs a
    // live plugin target, so it lives as an e2e in tests/plugin_host_ops.rs.
    // These cover the argument and service wiring failure modes.

    #[tokio::test]
    async fn open_view_missing_plugin_is_invalid_args() {
        let host = view_services(Some(Arc::new(MockHostIo::new())), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());

        let resp = handle_host_call(
            7,
            "hello",
            "host.open_view",
            Some(&json!({ "channel_id": 99 })),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn open_view_non_string_command_is_invalid_args() {
        let host = view_services(Some(Arc::new(MockHostIo::new())), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());

        let resp = handle_host_call(
            7,
            "hello",
            "host.open_view",
            Some(&json!({ "channel_id": 99, "plugin": "hello", "command": 7 })),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn open_view_non_object_args_is_invalid_args() {
        let host = view_services(Some(Arc::new(MockHostIo::new())), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());

        let resp = handle_host_call(
            7,
            "hello",
            "host.open_view",
            Some(&json!({ "channel_id": 99, "plugin": "hello", "args": "nope" })),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn open_view_without_io_is_host_unavailable() {
        let host = services(None, Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());
        let resp = handle_host_call(
            7,
            "hello",
            "host.open_view",
            None,
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn open_view_without_engine_is_host_unavailable() {
        // io present so the io check passes; the engine check fires.
        let host = services(Some(Arc::new(MockHostIo::new())), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());
        let resp = handle_host_call(
            7,
            "hello",
            "host.open_view",
            None,
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn open_view_without_manager_is_host_unavailable() {
        // io and engine present so their checks pass; the manager check fires.
        let host = view_services(Some(Arc::new(MockHostIo::new())), Some(sample_config()));
        let resp = handle_host_call(7, "hello", "host.open_view", None, Some(&host), None).await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn open_view_unknown_target_is_plugin_not_found() {
        let host = view_services(Some(Arc::new(MockHostIo::new())), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());
        let resp = handle_host_call(
            7,
            "hello",
            "host.open_view",
            Some(&json!({ "channel_id": 99, "plugin": "nope" })),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_err(resp, 7, "PluginNotFound");
    }

    #[test]
    fn parse_open_view_message_id_accepts_number_string_and_absent() {
        let (.., id) = parse_open_view(Some(&json!({
            "channel_id": 99, "plugin": "hello", "message_id": 42
        })))
        .expect("number id parses");
        assert_eq!(id, Some(42));

        let (.., id) = parse_open_view(Some(&json!({
            "channel_id": 99, "plugin": "hello", "message_id": "42"
        })))
        .expect("string id parses");
        assert_eq!(id, Some(42));

        let (.., id) = parse_open_view(Some(&json!({ "channel_id": 99, "plugin": "hello" })))
            .expect("absent id parses");
        assert_eq!(id, None, "no message_id means the placeholder flow");

        // A present id that is not an id is the fallback path, not an error:
        // the caller posts a placeholder instead of failing the op.
        let (.., id) = parse_open_view(Some(&json!({
            "channel_id": 99, "plugin": "hello", "message_id": "nope"
        })))
        .expect("malformed id parses");
        assert_eq!(id, None);
    }

    // ── open_modal ────────────────────────────────────────────────────────────
    // The modal spec grammar is pwr-ext's: a label-wrapped input text is the
    // smallest valid component. `style`, the length fields, and `required` are
    // non-defaulted there, so the fixture carries them explicitly.

    fn modal_spec() -> Value {
        json!({
            "custom_id": "hello:modal",
            "title": "Tell us",
            "components": [{
                "type": 18,
                "label": "Note",
                "component": {
                    "type": 4,
                    "style": 1,
                    "custom_id": "note",
                    "min_length": null,
                    "max_length": null,
                    "required": true
                }
            }]
        })
    }

    fn open_modal_args(modal: Value) -> Value {
        json!({
            "author_id": 7,
            "interaction_id": 42,
            "token": "token-1",
            "modal": modal
        })
    }

    #[tokio::test]
    async fn open_modal_routes_through_the_seam() {
        let mut mock = MockHostIo::new();
        mock.expect_open_modal()
            .with(eq(42_u64), eq("token-1"), eq(modal_spec()))
            .times(1)
            .returning(|_, _, _| Ok(()));
        let host = services(Some(Arc::new(mock)), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());

        let resp = handle_host_call(
            7,
            "hello",
            "host.open_modal",
            Some(&open_modal_args(modal_spec())),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_eq!(assert_ok(resp, 7), None);
    }

    #[tokio::test]
    async fn open_modal_binds_the_caller_as_the_route_owner() {
        let mut mock = MockHostIo::new();
        mock.expect_open_modal()
            .times(1)
            .returning(|_, _, _| Ok(()));
        let host = services(Some(Arc::new(mock)), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());

        let resp = handle_host_call(
            7,
            "zed",
            "host.open_modal",
            Some(&open_modal_args(modal_spec())),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_eq!(assert_ok(resp, 7), None);

        let binding = manager.take_modal(7).await.expect("route bound");
        assert_eq!(
            binding.owner, "zed",
            "the host-stamped caller owns the route"
        );
        assert_eq!(binding.custom_id, "hello:modal");
    }

    #[tokio::test]
    async fn open_modal_accepts_serenity_string_ids() {
        let mut mock = MockHostIo::new();
        mock.expect_open_modal()
            .with(eq(42_u64), eq("token-1"), eq(modal_spec()))
            .times(1)
            .returning(|_, _, _| Ok(()));
        let host = services(Some(Arc::new(mock)), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());

        let resp = handle_host_call(
            7,
            "hello",
            "host.open_modal",
            Some(&json!({
                "author_id": "7",
                "interaction_id": "42",
                "token": "token-1",
                "modal": modal_spec()
            })),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_eq!(assert_ok(resp, 7), None);
        assert!(manager.take_modal(7).await.is_ok());
    }

    #[tokio::test]
    async fn open_modal_io_failure_is_host_io_error_and_binds_no_route() {
        let mut mock = MockHostIo::new();
        mock.expect_open_modal().times(1).returning(|_, _, _| {
            Err(HostError::Serenity(serenity::Error::Http(
                serenity::HttpError::InvalidWebhook,
            )))
        });
        let host = services(Some(Arc::new(mock)), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());

        let resp = handle_host_call(
            7,
            "hello",
            "host.open_modal",
            Some(&open_modal_args(modal_spec())),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_err(resp, 7, "HostIoError");
        assert_eq!(
            manager.take_modal(7).await.unwrap_err(),
            ModalRouteError::NoBinding(7),
            "a failed open leaves no route behind"
        );
    }

    #[tokio::test]
    async fn open_modal_invalid_spec_is_invalid_args_before_the_seam() {
        // No io expectations: any seam call panics the mock, so this also
        // proves validation happens before the open is attempted.
        let mock = MockHostIo::new();
        let host = services(Some(Arc::new(mock)), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());
        let bad = json!({
            "custom_id": "hello:modal",
            "title": "T",
            "components": [{ "type": 99 }]
        });

        let resp = handle_host_call(
            7,
            "hello",
            "host.open_modal",
            Some(&open_modal_args(bad)),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn open_modal_missing_author_is_invalid_args() {
        let mock = MockHostIo::new();
        let host = services(Some(Arc::new(mock)), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());

        let resp = handle_host_call(
            7,
            "hello",
            "host.open_modal",
            Some(&json!({
                "interaction_id": 42,
                "token": "token-1",
                "modal": modal_spec()
            })),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn open_modal_without_io_is_host_unavailable() {
        let host = services(None, Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());
        let resp = handle_host_call(
            7,
            "hello",
            "host.open_modal",
            Some(&open_modal_args(modal_spec())),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn open_modal_without_manager_is_host_unavailable() {
        // io present so its check passes; the manager check fires.
        let mock = MockHostIo::new();
        let host = services(Some(Arc::new(mock)), Some(sample_config()));
        let resp = handle_host_call(
            7,
            "hello",
            "host.open_modal",
            Some(&open_modal_args(modal_spec())),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");
    }

    // ── list_plugins ───────────────────────────────────────────────────────────
    // The happy path needs a live plugin in the manager, so it spawns the
    // stubborn fixture; the failure mode is the manager-less spawn.

    #[tokio::test]
    async fn list_plugins_returns_the_running_names() {
        let manager = Arc::new(PluginManager::new(None, RespawnPolicy::default()));
        let stub =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stubborn_plugin.sh");
        manager
            .spawn("stubborn", stub, None, &[], &[])
            .await
            .expect("spawn stubborn fixture");

        let resp =
            handle_host_call(7, "hello", "host.list_plugins", None, None, Some(&manager)).await;
        assert_eq!(assert_ok(resp, 7), Some(json!({ "plugins": ["stubborn"] })));

        manager.unload("stubborn", &[]).await.expect("teardown");
    }

    #[tokio::test]
    async fn list_plugins_without_manager_is_host_unavailable() {
        let resp = handle_host_call(7, "hello", "host.list_plugins", None, None, None).await;
        assert_err(resp, 7, "HostUnavailable");
    }

    // ── stats ─────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn stats_routes_through_the_seam() {
        let mut mock = MockStatsSource::new();
        mock.expect_stats().times(1).returning(|| {
            Ok(HostStats {
                version: "0.4.2".into(),
                uptime_secs: 90_000,
                guild_count: 2,
                user_count: 1_500,
                latency_ms: 42,
                command_count: 12,
                memory_mb: 320.0,
            })
        });
        let handle = StatsHandle::default();
        handle.attach(Arc::new(mock));
        let host = HostServices {
            io: None,
            config: Some(sample_config()),
            kv: None,
            engine: None,
            stats: Arc::new(handle),
            feeds: None,
            voice: None,
            welcome: None,
            previews: None,
        };

        let resp = handle_host_call(7, "hello", "host.stats", None, Some(&host), None).await;
        assert_eq!(
            assert_ok(resp, 7),
            Some(json!({
                "version": "0.4.2",
                "uptime_secs": 90_000,
                "guild_count": 2,
                "user_count": 1_500,
                "latency_ms": 42,
                "command_count": 12,
                "memory_mb": 320.0,
            }))
        );
    }

    #[tokio::test]
    async fn stats_before_attachment_is_host_unavailable() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(7, "hello", "host.stats", None, Some(&host), None).await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn stats_without_services_is_host_unavailable() {
        let resp = handle_host_call(7, "hello", "host.stats", None, None, None).await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn stats_failure_is_stats_error() {
        let mut mock = MockStatsSource::new();
        mock.expect_stats().times(1).returning(|| {
            Err(StatsError::Http(serenity::Error::Http(
                serenity::HttpError::InvalidWebhook,
            )))
        });
        let handle = StatsHandle::default();
        handle.attach(Arc::new(mock));
        let host = HostServices {
            io: None,
            config: Some(sample_config()),
            kv: None,
            engine: None,
            stats: Arc::new(handle),
            feeds: None,
            voice: None,
            welcome: None,
            previews: None,
        };

        let resp = handle_host_call(7, "hello", "host.stats", None, Some(&host), None).await;
        let msg = assert_err(resp, 7, "StatsError");
        assert!(msg.contains("webhook"), "msg: {msg}");
    }

    // ── feed settings ─────────────────────────────────────────────────────────

    fn sample_settings() -> ServerSettings {
        ServerSettings {
            feeds: pwr_plugin_protocol::FeedsSettings {
                enabled: Some(true),
                channel_id: Some("123456789".into()),
                subscribe_role_id: Some("987654321".into()),
                unsubscribe_role_id: None,
            },
            ..ServerSettings::default()
        }
    }

    fn feed_services(feeds: Arc<dyn FeedSettingsSource>) -> HostServices {
        HostServices {
            io: None,
            config: Some(sample_config()),
            kv: None,
            engine: None,
            stats: Arc::new(StatsHandle::default()),
            feeds: Some(feeds),
            voice: None,
            welcome: None,
            previews: None,
        }
    }

    fn welcome_services(welcome: Arc<dyn WelcomeSettingsSource>) -> HostServices {
        HostServices {
            io: None,
            config: Some(sample_config()),
            kv: None,
            engine: None,
            stats: Arc::new(StatsHandle::default()),
            feeds: None,
            voice: None,
            welcome: Some(welcome),
            previews: None,
        }
    }

    #[tokio::test]
    async fn feed_get_settings_routes_through_the_seam() {
        let mut mock = MockFeedSettingsSource::new();
        mock.expect_get_settings()
            .with(eq(42u64))
            .times(1)
            .returning(|_| Ok(sample_settings()));
        let host = feed_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.feed.get_settings",
            Some(&json!({ "guild_id": 42 })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(
            assert_ok(resp, 7),
            Some(json!({
                "feeds": {
                    "enabled": true,
                    "channel_id": "123456789",
                    "subscribe_role_id": "987654321",
                    "unsubscribe_role_id": null,
                },
                "voice": { "enabled": null },
                "welcome": {
                    "enabled": null,
                    "channel_id": null,
                    "primary_color": null,
                    "template_id": null,
                    "messages": null,
                },
            }))
        );
    }

    #[tokio::test]
    async fn feed_update_settings_routes_through_the_seam() {
        let settings = sample_settings();
        let mut mock = MockFeedSettingsSource::new();
        mock.expect_update_settings()
            .with(eq(42u64), eq(settings.clone()))
            .times(1)
            .returning(|_, _| Ok(()));
        let host = feed_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.feed.update_settings",
            Some(&json!({ "guild_id": 42, "settings": settings })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), None);
    }

    #[tokio::test]
    async fn feed_settings_without_services_is_host_unavailable() {
        let resp = handle_host_call(
            7,
            "hello",
            "host.feed.get_settings",
            Some(&json!({ "guild_id": 42 })),
            None,
            None,
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");

        let resp = handle_host_call(
            7,
            "hello",
            "host.feed.update_settings",
            Some(&json!({ "guild_id": 42, "settings": ServerSettings::default() })),
            None,
            None,
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn feed_settings_without_source_is_host_unavailable() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.feed.get_settings",
            Some(&json!({ "guild_id": 42 })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn feed_settings_missing_guild_id_is_invalid_args() {
        let mock = MockFeedSettingsSource::new();
        let host = feed_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.feed.get_settings",
            None,
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn feed_settings_malformed_snapshot_is_invalid_args() {
        let mock = MockFeedSettingsSource::new();
        let host = feed_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.feed.update_settings",
            Some(&json!({ "guild_id": 42, "settings": "not-a-snapshot" })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn feed_settings_service_failure_is_feed_settings_error() {
        let mut mock = MockFeedSettingsSource::new();
        mock.expect_get_settings()
            .with(eq(42u64))
            .times(1)
            .returning(|_| {
                Err(FeedSettingsError::Service(ServiceError::UnexpectedResult {
                    message: "no such guild".into(),
                }))
            });
        let host = feed_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.feed.get_settings",
            Some(&json!({ "guild_id": 42 })),
            Some(&host),
            None,
        )
        .await;
        let msg = assert_err(resp, 7, "FeedSettingsError");
        assert!(msg.contains("no such guild"), "msg: {msg}");
    }

    #[tokio::test]
    async fn feed_settings_string_guild_id_parses() {
        let mut mock = MockFeedSettingsSource::new();
        mock.expect_get_settings()
            .with(eq(42u64))
            .times(1)
            .returning(|_| Ok(sample_settings()));
        let host = feed_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.feed.get_settings",
            Some(&json!({ "guild_id": "42" })),
            Some(&host),
            None,
        )
        .await;
        assert_ok(resp, 7);
    }

    // ── voice settings ────────────────────────────────────────────────────────

    fn voice_sample_settings() -> ServerSettings {
        ServerSettings {
            voice: pwr_plugin_protocol::VoiceSettings {
                enabled: Some(false),
            },
            ..ServerSettings::default()
        }
    }

    fn voice_services(voice: Arc<dyn VoiceSettingsSource>) -> HostServices {
        HostServices {
            io: None,
            config: Some(sample_config()),
            kv: None,
            engine: None,
            stats: Arc::new(StatsHandle::default()),
            feeds: None,
            voice: Some(voice),
            welcome: None,
            previews: None,
        }
    }

    #[tokio::test]
    async fn voice_get_settings_routes_through_the_seam() {
        let mut mock = MockVoiceSettingsSource::new();
        mock.expect_get_settings()
            .with(eq(42u64))
            .times(1)
            .returning(|_| Ok(voice_sample_settings()));
        let host = voice_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.voice.get_settings",
            Some(&json!({ "guild_id": 42 })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(
            assert_ok(resp, 7),
            Some(json!({
                "feeds": {
                    "enabled": null,
                    "channel_id": null,
                    "subscribe_role_id": null,
                    "unsubscribe_role_id": null,
                },
                "voice": { "enabled": false },
                "welcome": {
                    "enabled": null,
                    "channel_id": null,
                    "primary_color": null,
                    "template_id": null,
                    "messages": null,
                },
            }))
        );
    }

    #[tokio::test]
    async fn voice_update_settings_routes_through_the_seam() {
        let settings = voice_sample_settings();
        let mut mock = MockVoiceSettingsSource::new();
        mock.expect_update_settings()
            .with(eq(42u64), eq(settings.clone()))
            .times(1)
            .returning(|_, _| Ok(()));
        let host = voice_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.voice.update_settings",
            Some(&json!({ "guild_id": 42, "settings": settings })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), None);
    }

    #[tokio::test]
    async fn voice_settings_without_services_is_host_unavailable() {
        let resp = handle_host_call(
            7,
            "hello",
            "host.voice.get_settings",
            Some(&json!({ "guild_id": 42 })),
            None,
            None,
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");

        let resp = handle_host_call(
            7,
            "hello",
            "host.voice.update_settings",
            Some(&json!({ "guild_id": 42, "settings": ServerSettings::default() })),
            None,
            None,
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn voice_settings_without_source_is_host_unavailable() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.voice.get_settings",
            Some(&json!({ "guild_id": 42 })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn voice_settings_missing_guild_id_is_invalid_args() {
        let mock = MockVoiceSettingsSource::new();
        let host = voice_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.voice.get_settings",
            None,
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn voice_settings_malformed_snapshot_is_invalid_args() {
        let mock = MockVoiceSettingsSource::new();
        let host = voice_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.voice.update_settings",
            Some(&json!({ "guild_id": 42, "settings": "not-a-snapshot" })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn voice_settings_service_failure_is_voice_settings_error() {
        let mut mock = MockVoiceSettingsSource::new();
        mock.expect_get_settings()
            .with(eq(42u64))
            .times(1)
            .returning(|_| {
                Err(VoiceSettingsError::Service(anyhow::anyhow!(
                    "no such guild"
                )))
            });
        let host = voice_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.voice.get_settings",
            Some(&json!({ "guild_id": 42 })),
            Some(&host),
            None,
        )
        .await;
        let msg = assert_err(resp, 7, "VoiceSettingsError");
        assert!(msg.contains("no such guild"), "msg: {msg}");
    }

    #[tokio::test]
    async fn voice_settings_string_guild_id_parses() {
        let mut mock = MockVoiceSettingsSource::new();
        mock.expect_get_settings()
            .with(eq(42u64))
            .times(1)
            .returning(|_| Ok(voice_sample_settings()));
        let host = voice_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.voice.get_settings",
            Some(&json!({ "guild_id": "42" })),
            Some(&host),
            None,
        )
        .await;
        assert_ok(resp, 7);
    }

    // ── welcome settings ────────────────────────────────────────────────────

    fn welcome_sample_settings() -> ServerSettings {
        ServerSettings {
            welcome: pwr_plugin_protocol::WelcomeSettings {
                enabled: Some(true),
                channel_id: Some("123456789".into()),
                primary_color: Some("#5865F2".into()),
                template_id: Some("1".into()),
                messages: Some(vec!["hello {user}".into()]),
            },
            ..ServerSettings::default()
        }
    }

    #[tokio::test]
    async fn welcome_get_settings_routes_through_the_seam() {
        let mut mock = MockWelcomeSettingsSource::new();
        mock.expect_get_settings()
            .with(eq(42u64))
            .times(1)
            .returning(|_| Ok(welcome_sample_settings()));
        let host = welcome_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.welcome.get_settings",
            Some(&json!({ "guild_id": 42 })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(
            assert_ok(resp, 7),
            Some(json!({
                "feeds": {
                    "enabled": null,
                    "channel_id": null,
                    "subscribe_role_id": null,
                    "unsubscribe_role_id": null,
                },
                "voice": { "enabled": null },
                "welcome": {
                    "enabled": true,
                    "channel_id": "123456789",
                    "primary_color": "#5865F2",
                    "template_id": "1",
                    "messages": ["hello {user}"],
                },
            }))
        );
    }

    #[tokio::test]
    async fn welcome_update_settings_routes_through_the_seam() {
        let settings = welcome_sample_settings();
        let mut mock = MockWelcomeSettingsSource::new();
        mock.expect_update_settings()
            .with(eq(42u64), eq(settings.clone()))
            .times(1)
            .returning(|_, _| Ok(()));
        let host = welcome_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.welcome.update_settings",
            Some(&json!({ "guild_id": 42, "settings": settings })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), None);
    }

    #[tokio::test]
    async fn welcome_settings_without_services_is_host_unavailable() {
        let resp = handle_host_call(
            7,
            "hello",
            "host.welcome.get_settings",
            Some(&json!({ "guild_id": 42 })),
            None,
            None,
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");

        let resp = handle_host_call(
            7,
            "hello",
            "host.welcome.update_settings",
            Some(&json!({ "guild_id": 42, "settings": ServerSettings::default() })),
            None,
            None,
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn welcome_settings_without_source_is_host_unavailable() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.welcome.get_settings",
            Some(&json!({ "guild_id": 42 })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn welcome_settings_service_failure_is_welcome_settings_error() {
        let mut mock = MockWelcomeSettingsSource::new();
        mock.expect_get_settings()
            .with(eq(42u64))
            .times(1)
            .returning(|_| {
                Err(WelcomeSettingsError::Service(
                    ServiceError::UnexpectedResult {
                        message: "no such guild".into(),
                    },
                ))
            });
        let host = welcome_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.welcome.get_settings",
            Some(&json!({ "guild_id": 42 })),
            Some(&host),
            None,
        )
        .await;
        let msg = assert_err(resp, 7, "WelcomeSettingsError");
        assert!(msg.contains("no such guild"), "msg: {msg}");
    }

    #[tokio::test]
    async fn welcome_settings_string_guild_id_parses() {
        let mut mock = MockWelcomeSettingsSource::new();
        mock.expect_get_settings()
            .with(eq(42u64))
            .times(1)
            .returning(|_| Ok(welcome_sample_settings()));
        let host = welcome_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.welcome.get_settings",
            Some(&json!({ "guild_id": "42" })),
            Some(&host),
            None,
        )
        .await;
        assert_ok(resp, 7);
    }

    #[tokio::test]
    async fn welcome_settings_wrong_guild_id_type_is_invalid_args() {
        let mock = MockWelcomeSettingsSource::new();
        let host = welcome_services(Arc::new(mock));

        let resp = handle_host_call(
            7,
            "hello",
            "host.welcome.get_settings",
            Some(&json!({ "guild_id": {"id": 42} })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    // ── kv ────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn kv_get_returns_value_through_the_seam() {
        let mut mock = MockKvStore::new();
        mock.expect_get()
            .with(eq("settings"), eq("theme"))
            .times(1)
            .returning(|_, _| Ok(Some("dark".into())));
        let host = kv_services(None, Some(sample_config()), Some(Arc::new(mock)));

        let resp = handle_host_call(
            7,
            "hello",
            "host.kv.get",
            Some(&json!({ "namespace": "settings", "key": "theme" })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), Some(json!({ "value": "dark" })));
    }

    #[tokio::test]
    async fn kv_get_absent_key_returns_null_value() {
        let mut mock = MockKvStore::new();
        mock.expect_get()
            .with(eq("settings"), eq("theme"))
            .times(1)
            .returning(|_, _| Ok(None));
        let host = kv_services(None, Some(sample_config()), Some(Arc::new(mock)));

        let resp = handle_host_call(
            7,
            "hello",
            "host.kv.get",
            Some(&json!({ "namespace": "settings", "key": "theme" })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), Some(json!({ "value": null })));
    }

    #[tokio::test]
    async fn kv_set_routes_through_the_seam() {
        let mut mock = MockKvStore::new();
        mock.expect_set()
            .with(eq("settings"), eq("theme"), eq("light"))
            .times(1)
            .returning(|_, _, _| Ok(()));
        let host = kv_services(None, Some(sample_config()), Some(Arc::new(mock)));

        let resp = handle_host_call(
            7,
            "hello",
            "host.kv.set",
            Some(&json!({
                "namespace": "settings",
                "key": "theme",
                "value": "light",
            })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), None);
    }

    #[tokio::test]
    async fn kv_delete_routes_through_the_seam() {
        let mut mock = MockKvStore::new();
        mock.expect_delete()
            .with(eq("settings"), eq("theme"))
            .times(1)
            .returning(|_, _| Ok(()));
        let host = kv_services(None, Some(sample_config()), Some(Arc::new(mock)));

        let resp = handle_host_call(
            7,
            "hello",
            "host.kv.delete",
            Some(&json!({ "namespace": "settings", "key": "theme" })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), None);
    }

    #[tokio::test]
    async fn kv_ops_forward_the_namespace_verbatim() {
        let mut mock = MockKvStore::new();
        mock.expect_get()
            .with(eq("settings"), eq("theme"))
            .times(1)
            .returning(|_, _| Ok(Some("dark".into())));
        mock.expect_get()
            .with(eq("other"), eq("theme"))
            .times(1)
            .returning(|_, _| Ok(None));
        let host = kv_services(None, Some(sample_config()), Some(Arc::new(mock)));

        let resp = handle_host_call(
            7,
            "hello",
            "host.kv.get",
            Some(&json!({ "namespace": "settings", "key": "theme" })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), Some(json!({ "value": "dark" })));

        let resp = handle_host_call(
            7,
            "hello",
            "host.kv.get",
            Some(&json!({ "namespace": "other", "key": "theme" })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), Some(json!({ "value": null })));
    }

    #[tokio::test]
    async fn kv_get_missing_key_is_invalid_args() {
        let mock = MockKvStore::new();
        let host = kv_services(None, Some(sample_config()), Some(Arc::new(mock)));

        let resp = handle_host_call(
            7,
            "hello",
            "host.kv.get",
            Some(&json!({ "namespace": "settings" })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn kv_set_missing_value_is_invalid_args() {
        let mock = MockKvStore::new();
        let host = kv_services(None, Some(sample_config()), Some(Arc::new(mock)));

        let resp = handle_host_call(
            7,
            "hello",
            "host.kv.set",
            Some(&json!({ "namespace": "settings", "key": "theme" })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn kv_op_without_kv_is_kv_unavailable() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.kv.get",
            Some(&json!({ "namespace": "settings", "key": "theme" })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "KvUnavailable");
    }

    #[tokio::test]
    async fn kv_store_failure_is_kv_store_error() {
        let mut mock = MockKvStore::new();
        mock.expect_get().times(1).returning(|_, _| {
            Err(KvError::Database(
                crate::repo::error::DatabaseError::PoolError("boom".into()),
            ))
        });
        let host = kv_services(None, Some(sample_config()), Some(Arc::new(mock)));

        let resp = handle_host_call(
            7,
            "hello",
            "host.kv.get",
            Some(&json!({ "namespace": "settings", "key": "theme" })),
            Some(&host),
            None,
        )
        .await;
        let msg = assert_err(resp, 7, "KvStoreError");
        assert!(msg.contains("boom"), "msg: {msg}");
    }

    // ── seam failures are first-class wire errors ─────────────────────────────

    #[tokio::test]
    async fn seam_failure_is_host_io_error() {
        let mut mock = MockHostIo::new();
        mock.expect_send_message().times(1).returning(|_, _| {
            Err(HostError::Serenity(serenity::Error::Http(
                serenity::HttpError::InvalidWebhook,
            )))
        });
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "hello",
            "host.send_message",
            Some(&json!({ "channel_id": 99, "content": "hi" })),
            Some(&host),
            None,
        )
        .await;
        let msg = assert_err(resp, 7, "HostIoError");
        assert!(msg.contains("webhook"), "msg: {msg}");
    }

    // ── config conversion ─────────────────────────────────────────────────────

    #[test]
    fn host_config_from_bot_config_copies_the_three_fields() {
        let mut bot = crate::config::Config::new();
        bot.db_url = "postgres://bot".into();
        bot.data_path = PathBuf::from("/var/lib/pwr-bot");
        bot.poll_interval = Duration::from_secs(60);

        let config = HostConfig::from(&bot);
        assert_eq!(config.db_url, "postgres://bot");
        assert_eq!(config.data_path, PathBuf::from("/var/lib/pwr-bot"));
        assert_eq!(config.poll_interval, Duration::from_secs(60));
    }
}
