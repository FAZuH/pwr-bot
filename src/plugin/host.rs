//! Host capability ops: the `host.*` calls a plugin may make on the host.
//!
//! The plugin announces the ops it needs in its hello `caps`; the host serves
//! them as plugin→host [`Msg::Call`]s. All Discord I/O (defer, acknowledge,
//! send/edit message) lives behind the [`HostIo`] trait so tests can drive it
//! with a mock instead of a live gateway. `host.get_config` is pure data and
//! never touches the seam.
//!
//! Ops not in the v1 surface (unknown `host.*` strings, and non-`host.*` ops
//! like `invoke`, which only ever travel host→plugin) answer
//! `UnknownOp`; v1 ops that are not implemented yet ([`HostCap::KvGet`],
//! [`HostCap::KvSet`], [`HostCap::KvDelete`], [`HostCap::OpenView`]) answer
//! `Unsupported`; and a missing service answers `HostUnavailable` /
//! `ConfigUnavailable` instead of panicking.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mockall::automock;
use poise::serenity_prelude as serenity;
use pwr_plugin_protocol::HostCap;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::WireError;
use serde_json::Value;
use serde_json::json;

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

/// What the host can serve a plugin: the Discord I/O seam and the config
/// subset. Both are optional so a plugin can be spawned without either and
/// still answer `UnknownOp`/`Unavailable` cleanly.
#[derive(Clone, Default)]
pub struct HostServices {
    /// Discord I/O seam, absent when the host is not wired to Discord.
    pub io: Option<Arc<dyn HostIo>>,
    /// Host config subset, absent when the host has no config to serve.
    pub config: Option<HostConfig>,
}

/// Serves one plugin→host [`Msg::Call`], answering with the correlation-id
/// matched `resp`. Every failure crosses the wire as a first-class
/// [`WireError`]; nothing panics on unknown ops or missing services.
pub async fn handle_host_call(
    id: u64,
    op: &str,
    args: Option<&Value>,
    host: Option<&HostServices>,
) -> Msg {
    let Some(cap) = HostCap::parse(op) else {
        return resp_err(
            id,
            "UnknownOp",
            format!("unknown op `{op}` (non-host.* and unknown host.* ops are not served)"),
        );
    };
    match cap {
        HostCap::KvGet | HostCap::KvSet | HostCap::KvDelete | HostCap::OpenView => resp_err(
            id,
            "Unsupported",
            format!("host op `{op}` is not implemented (later ticket)"),
        ),
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

    fn services(io: Option<Arc<dyn HostIo>>, config: Option<HostConfig>) -> HostServices {
        HostServices { io, config }
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
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    #[tokio::test]
    async fn defer_without_services_is_host_unavailable() {
        let resp = handle_host_call(7, "host.defer", None, None).await;
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
        )
        .await;
        assert_err(resp, 7, "InvalidArgs");
    }

    // ── get_config ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn get_config_returns_the_three_fields() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(7, "host.get_config", None, Some(&host)).await;
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

        let resp = handle_host_call(7, "host.get_config", None, Some(&host)).await;
        assert_err(resp, 7, "ConfigUnavailable");
    }

    // ── unknown / unsupported ops ─────────────────────────────────────────────

    #[tokio::test]
    async fn unknown_host_op_is_unknown_op() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(7, "host.frobnicate", None, Some(&host)).await;
        let msg = assert_err(resp, 7, "UnknownOp");
        assert!(msg.contains("host.frobnicate"), "msg: {msg}");
    }

    #[tokio::test]
    async fn non_host_op_is_unknown_op() {
        let host = services(None, Some(sample_config()));

        let resp = handle_host_call(7, "invoke", None, Some(&host)).await;
        assert_err(resp, 7, "UnknownOp");
    }

    #[tokio::test]
    async fn kv_and_view_ops_are_unsupported() {
        let host = services(None, Some(sample_config()));
        for op in [
            "host.kv.get",
            "host.kv.set",
            "host.kv.delete",
            "host.open_view",
        ] {
            let resp = handle_host_call(7, op, None, Some(&host)).await;
            let msg = assert_err(resp, 7, "Unsupported");
            assert!(msg.contains(op), "msg for {op}: {msg}");
        }
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
