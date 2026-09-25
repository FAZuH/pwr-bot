//! Host op surface: the operations a plugin may require from the host.
//!
//! A plugin declares the host ops it needs in its hello `ops` list. Only
//! `host.*`-prefixed entries are validated against the v2 surface; anything
//! else (e.g. `command:feed`) is an operation the plugin itself serves and is
//! opaque to the host. A plugin declaring an unknown `host.*` op is rejected
//! at spawn. Logging needs no op: stderr is the free logging channel.

use serde::Deserialize;
use serde::Serialize;

/// A host op a plugin may require (v2 surface). The serde representation is
/// the wire string, e.g. `HostOp::KvGet` ↔ `"host.kv.get"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostOp {
    /// Defer the interaction response (long-running command).
    Defer,
    /// Send a message to a channel. The prose renders as a text display
    /// inside a Components V2 envelope; a raw Discord message payload rides
    /// the `data` argument verbatim after the host's validate-only gate
    /// (ADR-0003), with optional `files` entries for runtime-generated
    /// attachments. `files` requires `data`: a prose-only send cannot carry
    /// files.
    SendMessage,
    /// Open or resolve a user's DM channel and return its channel id.
    OpenDm,
    /// Edit a previously sent message.
    EditMessage,
    /// Acknowledge an interaction without a visible reply.
    Acknowledge,
    /// Read a key from the host's plugin key-value store.
    KvGet,
    /// Write a key to the host's plugin key-value store.
    KvSet,
    /// Delete a key from the host's plugin key-value store.
    KvDelete,
    /// Open another plugin's view (cross-plugin navigation).
    OpenView,
    /// Open a modal in response to an interaction and receive the
    /// author-keyed submission on the owning session as a correlated
    /// `view.modal_submit` call. The open rides the interaction's
    /// id+token pair, so the interaction must be unanswered — the open IS
    /// the response (ADR-0007). The click path answers interactions with
    /// the plugin's view reply (type 7) when it arrives in time and
    /// otherwise falls back to ack + webhook edit, so an unanswered
    /// interaction only survives when a plugin answers with an
    /// `host.open_modal` effect — which is then the response itself.
    OpenModal,
    /// Fetch host configuration (db url, data path, poll interval).
    GetConfig,
    /// List the names of all running plugins.
    ListPlugins,
    /// Read the live bot statistics shown by the host's `/about` command.
    Stats,
    /// Resolve user display data through the host's cache and bounded REST
    /// fallback.
    ResolveUsers,
    /// Read a guild's welcome settings (the whole [`crate::ServerSettings`]
    /// snapshot), mirroring the welcome service's `get_server_settings`.
    WelcomeGetSettings,
    /// Write a guild's welcome settings snapshot, mirroring the welcome
    /// service's `update_server_settings`.
    WelcomeUpdateSettings,
}

/// Every op in the v2 host op surface, in declaration order. The
/// canonical set: [`HostOp::parse`] accepts exactly these ops, and the serde
/// representation derives from [`HostOp::as_str`], so adding an op touches
/// this list, the enum, and `as_str` — nowhere else.
pub const ALL_OPS: &[HostOp] = &[
    HostOp::Defer,
    HostOp::SendMessage,
    HostOp::OpenDm,
    HostOp::EditMessage,
    HostOp::Acknowledge,
    HostOp::KvGet,
    HostOp::KvSet,
    HostOp::KvDelete,
    HostOp::OpenView,
    HostOp::OpenModal,
    HostOp::GetConfig,
    HostOp::ListPlugins,
    HostOp::Stats,
    HostOp::ResolveUsers,
    HostOp::WelcomeGetSettings,
    HostOp::WelcomeUpdateSettings,
];

impl HostOp {
    /// The wire string for this op, e.g. `HostOp::KvGet` ↔ `"host.kv.get"`.
    /// The single source of the wire mapping: [`HostOp::parse`] and the
    /// serde representation both derive from it.
    pub fn as_str(&self) -> &'static str {
        match self {
            HostOp::Defer => "host.defer",
            HostOp::SendMessage => "host.send_message",
            HostOp::OpenDm => "host.open_dm",
            HostOp::EditMessage => "host.edit_message",
            HostOp::Acknowledge => "host.acknowledge",
            HostOp::KvGet => "host.kv.get",
            HostOp::KvSet => "host.kv.set",
            HostOp::KvDelete => "host.kv.delete",
            HostOp::OpenView => "host.open_view",
            HostOp::OpenModal => "host.open_modal",
            HostOp::GetConfig => "host.get_config",
            HostOp::ListPlugins => "host.list_plugins",
            HostOp::Stats => "host.stats",
            HostOp::ResolveUsers => "host.resolve_users",
            HostOp::WelcomeGetSettings => "host.welcome.get_settings",
            HostOp::WelcomeUpdateSettings => "host.welcome.update_settings",
        }
    }

    /// Parses an op string into a host op. Returns `None` for anything that is
    /// not one of the v2 `host.*` ops — including non-`host.*` ops such as
    /// `command:feed`, which the host treats as opaque.
    pub fn parse(wire: &str) -> Option<HostOp> {
        ALL_OPS.iter().find(|op| op.as_str() == wire).copied()
    }
}

impl Serialize for HostOp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for HostOp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = String::deserialize(deserializer)?;
        HostOp::parse(&wire)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown host op `{wire}`")))
    }
}

/// The host's bounded user projection returned by `host.resolve_users`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResolvedUser {
    /// Discord user id.
    pub id: u64,
    /// Global Discord username.
    pub name: String,
    /// Guild-aware display name when the request included a guild.
    pub display_name: String,
    /// Avatar URL selected by the host's cache/REST path.
    pub avatar_url: String,
    /// Whether the Discord account is a bot.
    pub is_bot: bool,
    /// Whether the user is a member of the requested guild.
    pub is_member: bool,
}

/// An unknown `host.*` op declared in a plugin's `ops`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown host op `{op}`")]
pub struct OpsError {
    /// The offending op string, e.g. `host.frobnicate`.
    pub op: String,
}

/// Validates a plugin's `ops` list (the [`crate::msg::Msg::Hello`] field)
/// against the v2 host op surface. Non-`host.*` entries — commands
/// the plugin serves, e.g. `command:feed` — are opaque and pass through
/// unvalidated; any `host.*` entry not in the v2 set is rejected. Returns the
/// declared host ops in order, so the host can check requirements without
/// re-parsing strings.
pub fn validate_ops(ops: &[String]) -> Result<Vec<HostOp>, OpsError> {
    let mut host_ops = Vec::new();
    for op in ops {
        if op.starts_with("host.") {
            host_ops.push(HostOp::parse(op).ok_or_else(|| OpsError { op: op.clone() })?);
        }
    }
    Ok(host_ops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_v2_op_parses() {
        for op in ALL_OPS {
            assert_eq!(
                HostOp::parse(op.as_str()),
                Some(*op),
                "op `{}`",
                op.as_str()
            );
        }
    }

    #[test]
    fn all_ops_is_exactly_the_v2_surface() {
        assert_eq!(ALL_OPS.len(), 16);
        let mut seen = std::collections::HashSet::new();
        for op in ALL_OPS {
            assert!(seen.insert(*op), "duplicate op in ALL_OPS");
        }
    }

    #[test]
    fn open_dm_op_parses() {
        assert_eq!(HostOp::parse("host.open_dm"), Some(HostOp::OpenDm));
    }

    #[test]
    fn retired_plugin_host_ops_do_not_parse() {
        assert_eq!(HostOp::parse("host.feed.get_settings"), None);
        assert_eq!(HostOp::parse("host.feed.update_settings"), None);
        assert_eq!(HostOp::parse("host.voice.get_settings"), None);
        assert_eq!(HostOp::parse("host.voice.update_settings"), None);
    }

    #[test]
    fn unknown_host_op_does_not_parse() {
        assert_eq!(HostOp::parse("host.frobnicate"), None);
        assert_eq!(HostOp::parse("host."), None);
        assert_eq!(HostOp::parse("host"), None);
    }

    #[test]
    fn non_host_op_does_not_parse() {
        assert_eq!(HostOp::parse("command:feed"), None);
        assert_eq!(HostOp::parse(""), None);
    }

    #[test]
    fn each_op_serializes_to_its_wire_string() {
        for op in ALL_OPS {
            let wire = op.as_str();
            let json = serde_json::to_string(op).unwrap();
            assert_eq!(json, format!(r#""{wire}""#), "op `{wire}`");
        }
    }

    #[test]
    fn each_op_deserializes_from_its_wire_string() {
        for op in ALL_OPS {
            let wire = op.as_str();
            let json = format!(r#""{wire}""#);
            assert_eq!(
                serde_json::from_str::<HostOp>(&json).unwrap(),
                *op,
                "op `{wire}`"
            );
        }
    }

    #[test]
    fn unknown_wire_string_fails_to_deserialize() {
        assert!(serde_json::from_str::<HostOp>(r#""host.frobnicate""#).is_err());
    }

    #[test]
    fn mixed_ops_validate_and_return_declared_host_ops() {
        let ops = vec![
            "command:feed.list".into(),
            "host.kv.get".into(),
            "host.kv.set".into(),
            "host.open_view".into(),
        ];
        assert_eq!(
            validate_ops(&ops),
            Ok(vec![HostOp::KvGet, HostOp::KvSet, HostOp::OpenView])
        );
    }

    #[test]
    fn hello_style_ops_validate() {
        let hello = crate::msg::Msg::Hello {
            v: crate::msg::API_VERSION,
            name: "feed".into(),
            ops: vec!["command:feed".into(), "host.send_message".into()],
            manifest: None,
        };
        let crate::msg::Msg::Hello { ops, .. } = hello else {
            unreachable!()
        };
        assert_eq!(validate_ops(&ops), Ok(vec![HostOp::SendMessage]));
    }

    #[test]
    fn unknown_host_op_is_rejected() {
        let ops = vec!["command:feed".into(), "host.frobnicate".into()];
        assert_eq!(
            validate_ops(&ops),
            Err(OpsError {
                op: "host.frobnicate".into()
            })
        );
    }

    #[test]
    fn empty_ops_validate_to_empty() {
        assert_eq!(validate_ops(&[]), Ok(vec![]));
    }

    #[test]
    fn non_host_ops_alone_validate_to_empty() {
        let ops = vec!["command:feed".into(), "command:feed.subscribe".into()];
        assert_eq!(validate_ops(&ops), Ok(vec![]));
    }
}
