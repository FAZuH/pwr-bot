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
use std::time::Duration;

use async_trait::async_trait;
use log::debug;
use log::warn;
use mockall::automock;
use poise::serenity_prelude as serenity;
use pwr_plugin_protocol::HostCap;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::WireError;
use serde_json::Value;
use serde_json::json;

use crate::plugin::InteractionEngine;
use crate::plugin::InteractionError;
use crate::plugin::PluginManager;
use crate::plugin::RunningPlugin;

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

    /// Sends a message to a channel, optionally carrying extra payload fields
    /// beyond `content`. Returns the created message's id.
    async fn send_message(
        &self,
        channel_id: u64,
        content: &str,
        data: Option<Value>,
    ) -> Result<Option<Value>, HostError>;

    /// Edits a previously sent message in place. `data` is the full Discord
    /// message payload. Returns the edited message's id.
    async fn edit_message(
        &self,
        channel_id: u64,
        message_id: u64,
        data: Value,
    ) -> Result<Option<Value>, HostError>;
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
        data: Option<Value>,
    ) -> Result<Option<Value>, HostError> {
        let mut payload = json!({ "content": content });
        if let Some(Value::Object(fields)) = data {
            for (key, value) in fields {
                payload[key] = value;
            }
        }
        let message = self
            .http
            .send_message(channel_id.into(), Vec::new(), &payload)
            .await?;
        Ok(Some(json!({ "message_id": message.id.get() })))
    }

    async fn edit_message(
        &self,
        channel_id: u64,
        message_id: u64,
        data: Value,
    ) -> Result<Option<Value>, HostError> {
        let message = self
            .http
            .edit_message(channel_id.into(), message_id.into(), &data, Vec::new())
            .await?;
        Ok(Some(json!({ "message_id": message.id.get() })))
    }
}

/// An error from a [`HostIo`] operation.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    /// The Discord API rejected the request.
    #[error("discord api error: {0}")]
    Serenity(#[from] serenity::Error),
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

/// What the host can serve a plugin: the Discord I/O seam, the config subset,
/// the key-value store, and the interaction engine used to open
/// `host.open_view` sessions. All are optional so a plugin can be spawned
/// without any of them and still answer `UnknownOp`/`Unavailable` cleanly.
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
}

/// Serves one plugin→host [`Msg::Call`], answering with the correlation-id
/// matched `resp`. Every failure crosses the wire as a first-class
/// [`WireError`]; nothing panics on unknown ops or missing services. The
/// plugin manager resolves targets for `host.open_view`; it is optional so
/// plain spawns without a manager still answer `HostUnavailable`.
pub async fn handle_host_call(
    id: u64,
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
            match open_view_call(args, &*io, &engine, manager).await {
                Ok(data) => Msg::resp_ok(id, data),
                Err(wire) => Msg::resp_err(id, wire),
            }
        }
    }
}

/// Runs one I/O-backed host op against the seam and turns the outcome into a
/// wire value: `Ok(data)` for a successful `resp_ok`, `Err(wire)` for a
/// failed `resp_err` (`InvalidArgs` or `HostIoError`).
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
            let (channel_id, content, data) = parse_send_message(args)?;
            io.send_message(channel_id, &content, data)
                .await
                .map_err(host_io_err)
        }
        HostCap::EditMessage => {
            let (channel_id, message_id, data) = parse_edit_message(args)?;
            io.edit_message(channel_id, message_id, data)
                .await
                .map_err(host_io_err)
        }
        _ => unreachable!("io_call only receives I/O-backed ops"),
    }
}

/// Runs the `host.open_view` op end to end: resolves the target plugin from
/// the manager, posts a placeholder through the io seam, renders the target's
/// panel into an interaction-engine session on the produced message id, and
/// edits the placeholder to the final payload. Returns the produced message
/// id so the caller can route interactions on it.
async fn open_view_call(
    args: Option<&Value>,
    io: &dyn HostIo,
    engine: &InteractionEngine<RunningPlugin>,
    manager: &PluginManager,
) -> Result<Option<Value>, WireError> {
    let (channel_id, plugin_name, command, call_args) = parse_open_view(args)?;
    let Some(target) = manager.get(&plugin_name).await else {
        return Err(WireError {
            kind: "PluginNotFound".into(),
            msg: format!("target plugin `{plugin_name}` is not running"),
        });
    };
    let placeholder = io
        .send_message(channel_id, "Loading…", None)
        .await
        .map_err(host_io_err)?;
    let message_id = match placeholder {
        Some(data) => data
            .get("message_id")
            .and_then(Value::as_u64)
            .ok_or_else(|| WireError {
                kind: "HostIoError".into(),
                msg: "send_message resp carried no message id".into(),
            })?,
        None => {
            return Err(WireError {
                kind: "HostIoError".into(),
                msg: "send_message resp carried no message id".into(),
            });
        }
    };
    let spec = match engine
        .open(
            serenity::MessageId::new(message_id),
            target,
            &command,
            call_args,
        )
        .await
    {
        Ok(spec) => spec,
        Err(e) => {
            // No session was opened, so there is nothing to abandon; the
            // placeholder stays live as a stale message. Stray clicks on it
            // hit NoSession and are dropped by the router, like any other
            // dead session.
            debug!("open_view left placeholder for message {message_id} with no session: {e}");
            return Err(open_view_err(e));
        }
    };
    if let Err(e) = io.edit_message(channel_id, message_id, spec.data).await {
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
        InteractionError::NoSession { .. } => WireError {
            kind: "NoSession".into(),
            msg: err.to_string(),
        },
    }
}

/// Runs one KV-backed host op against the seam and turns the outcome into a
/// wire value: `Ok(data)` for a successful `resp_ok`, `Err(wire)` for a
/// failed `resp_err` (`InvalidArgs` or `KvStoreError`). An absent key is not
/// an error: `host.kv.get` answers `{"value": null}` so the plugin can apply
/// its default.
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
        _ => unreachable!("kv_call only receives KV-backed ops"),
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

/// Parses `host.send_message` args: a channel id, content, and optional extra
/// payload fields.
fn parse_send_message(args: Option<&Value>) -> Result<(u64, String, Option<Value>), WireError> {
    let obj = args.and_then(Value::as_object).ok_or_else(|| {
        invalid_args("expected args object with `channel_id` (u64) and `content` (string)")
    })?;
    let channel_id = obj
        .get("channel_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_args("missing `channel_id` (u64)"))?;
    let content = obj
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_args("missing `content` (string)"))?
        .to_string();
    let data = match obj.get("data") {
        None => None,
        Some(data) if data.is_object() => Some(data.clone()),
        Some(_) => return Err(invalid_args("`data` must be an object")),
    };
    Ok((channel_id, content, data))
}

/// Parses `host.edit_message` args: a channel id, message id, and the full
/// payload to write.
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
/// name, the target command (defaults to the plugin name), and the invoke
/// args forwarded to the target (defaults to `{}`).
fn parse_open_view(args: Option<&Value>) -> Result<(u64, String, String, Value), WireError> {
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
    Ok((channel_id, plugin, command, call_args))
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

    use mockall::predicate::eq;
    use serde_json::json;

    use super::*;
    use crate::plugin::RespawnPolicy;

    fn services(io: Option<Arc<dyn HostIo>>, config: Option<HostConfig>) -> HostServices {
        HostServices {
            io,
            config,
            kv: None,
            engine: None,
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
        let resp = handle_host_call(7, "host.defer", None, None, None).await;
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
            .with(
                eq(99_u64),
                eq("hello world"),
                eq(Some(json!({ "flags": 0 }))),
            )
            .times(1)
            .returning(|_, _, _| Ok(Some(json!({ "message_id": 1234 }))));
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
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
    async fn send_message_missing_content_is_invalid_args() {
        let mock = MockHostIo::new();
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "host.send_message",
            Some(&json!({ "channel_id": 99 })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn send_message_non_object_data_is_invalid_args() {
        let mock = MockHostIo::new();
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "host.send_message",
            Some(&json!({ "channel_id": 99, "content": "x", "data": "oops" })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    // ── edit_message ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn edit_message_routes_through_the_seam() {
        let mut mock = MockHostIo::new();
        mock.expect_edit_message()
            .with(eq(99_u64), eq(1234_u64), eq(json!({ "content": "edited" })))
            .times(1)
            .returning(|_, _, _| Ok(Some(json!({ "message_id": 1234 }))));
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
            "host.edit_message",
            Some(&json!({
                "channel_id": 99,
                "message_id": 1234,
                "data": { "content": "edited" },
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
            "host.edit_message",
            Some(&json!({ "channel_id": 99, "message_id": 1234 })),
            Some(&host),
            None,
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    // ── get_config ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn get_config_returns_the_three_fields() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(7, "host.get_config", None, Some(&host), None).await;
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

        let resp = handle_host_call(7, "host.get_config", None, Some(&host), None).await;
        assert_err(resp, 7, "ConfigUnavailable");
    }

    // ── unknown / unsupported ops ─────────────────────────────────────────────

    #[tokio::test]
    async fn unknown_host_op_is_unknown_op() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(7, "host.frobnicate", None, Some(&host), None).await;
        let msg = assert_err(resp, 7, "UnknownOp");
        assert!(msg.contains("host.frobnicate"), "msg: {msg}");
    }

    #[tokio::test]
    async fn non_host_op_is_unknown_op() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(7, "invoke", None, Some(&host), None).await;
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
        let resp = handle_host_call(7, "host.open_view", None, Some(&host), Some(&manager)).await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn open_view_without_engine_is_host_unavailable() {
        // io present so the io check passes; the engine check fires.
        let host = services(Some(Arc::new(MockHostIo::new())), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());
        let resp = handle_host_call(7, "host.open_view", None, Some(&host), Some(&manager)).await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn open_view_without_manager_is_host_unavailable() {
        // io and engine present so their checks pass; the manager check fires.
        let host = view_services(Some(Arc::new(MockHostIo::new())), Some(sample_config()));
        let resp = handle_host_call(7, "host.open_view", None, Some(&host), None).await;
        assert_err(resp, 7, "HostUnavailable");
    }

    #[tokio::test]
    async fn open_view_unknown_target_is_plugin_not_found() {
        let host = view_services(Some(Arc::new(MockHostIo::new())), Some(sample_config()));
        let manager = PluginManager::new(None, RespawnPolicy::default());
        let resp = handle_host_call(
            7,
            "host.open_view",
            Some(&json!({ "channel_id": 99, "plugin": "nope" })),
            Some(&host),
            Some(&manager),
        )
        .await;
        assert_err(resp, 7, "PluginNotFound");
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
            "host.kv.get",
            Some(&json!({ "namespace": "settings", "key": "theme" })),
            Some(&host),
            None,
        )
        .await;
        assert_eq!(assert_ok(resp, 7), Some(json!({ "value": "dark" })));

        let resp = handle_host_call(
            7,
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
        mock.expect_send_message().times(1).returning(|_, _, _| {
            Err(HostError::Serenity(serenity::Error::Http(
                serenity::HttpError::InvalidWebhook,
            )))
        });
        let host = services(Some(Arc::new(mock)), Some(sample_config()));

        let resp = handle_host_call(
            7,
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
