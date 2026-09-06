//! Host capability surface: the ops a plugin may require from the host.
//!
//! A plugin declares the host ops it needs in its hello `caps` list. Only
//! `host.*`-prefixed entries are validated against the v1 surface; anything
//! else (e.g. `command:feed`) is a capability the plugin itself serves and is
//! opaque to the host. A plugin declaring an unknown `host.*` op is rejected
//! at spawn. Logging needs no op: stderr is the free logging channel.

use serde::Deserialize;
use serde::Serialize;

/// A host op a plugin may require (v1 surface). The serde representation is
/// the wire string, e.g. `HostCap::KvGet` ↔ `"host.kv.get"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCap {
    /// Defer the interaction response (long-running command).
    Defer,
    /// Send a message to a channel. The prose renders as a Components V2
    /// text display; a legacy `data` argument is accepted and ignored.
    SendMessage,
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
    /// Fetch host configuration (db url, data path, poll interval).
    GetConfig,
    /// List the names of all running plugins.
    ListPlugins,
    /// Read the live bot statistics shown by the host's `/about` command.
    Stats,
    /// Read a guild's feed settings (the whole [`crate::ServerSettings`]
    /// snapshot), mirroring the feed service's `get_server_settings`.
    FeedGetSettings,
    /// Write a guild's feed settings snapshot, mirroring the feed service's
    /// `update_server_settings`.
    FeedUpdateSettings,
    /// Read a guild's voice settings (the whole [`crate::ServerSettings`]
    /// snapshot), mirroring the voice service's `get_server_settings`.
    VoiceGetSettings,
    /// Write a guild's voice settings snapshot, mirroring the voice service's
    /// `update_server_settings`.
    VoiceUpdateSettings,
}

/// Every op in the v1 host capability surface, in declaration order. The
/// canonical set: [`HostCap::parse`] accepts exactly these ops, and the serde
/// representation derives from [`HostCap::as_str`], so adding an op touches
/// this list, the enum, and `as_str` — nowhere else.
pub const ALL_CAPS: &[HostCap] = &[
    HostCap::Defer,
    HostCap::SendMessage,
    HostCap::EditMessage,
    HostCap::Acknowledge,
    HostCap::KvGet,
    HostCap::KvSet,
    HostCap::KvDelete,
    HostCap::OpenView,
    HostCap::GetConfig,
    HostCap::ListPlugins,
    HostCap::Stats,
    HostCap::FeedGetSettings,
    HostCap::FeedUpdateSettings,
    HostCap::VoiceGetSettings,
    HostCap::VoiceUpdateSettings,
];

impl HostCap {
    /// The wire string for this op, e.g. `HostCap::KvGet` ↔ `"host.kv.get"`.
    /// The single source of the wire mapping: [`HostCap::parse`] and the
    /// serde representation both derive from it.
    pub fn as_str(&self) -> &'static str {
        match self {
            HostCap::Defer => "host.defer",
            HostCap::SendMessage => "host.send_message",
            HostCap::EditMessage => "host.edit_message",
            HostCap::Acknowledge => "host.acknowledge",
            HostCap::KvGet => "host.kv.get",
            HostCap::KvSet => "host.kv.set",
            HostCap::KvDelete => "host.kv.delete",
            HostCap::OpenView => "host.open_view",
            HostCap::GetConfig => "host.get_config",
            HostCap::ListPlugins => "host.list_plugins",
            HostCap::Stats => "host.stats",
            HostCap::FeedGetSettings => "host.feed.get_settings",
            HostCap::FeedUpdateSettings => "host.feed.update_settings",
            HostCap::VoiceGetSettings => "host.voice.get_settings",
            HostCap::VoiceUpdateSettings => "host.voice.update_settings",
        }
    }

    /// Parses a cap string into a host op. Returns `None` for anything that is
    /// not one of the v1 `host.*` ops — including non-`host.*` caps such as
    /// `command:feed`, which the host treats as opaque.
    pub fn parse(wire: &str) -> Option<HostCap> {
        ALL_CAPS.iter().find(|cap| cap.as_str() == wire).copied()
    }
}

impl Serialize for HostCap {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for HostCap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = String::deserialize(deserializer)?;
        HostCap::parse(&wire)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown host capability `{wire}`")))
    }
}

/// An unknown `host.*` op declared in a plugin's `caps`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown host capability `{op}`")]
pub struct CapsError {
    /// The offending cap string, e.g. `host.frobnicate`.
    pub op: String,
}

/// Validates a plugin's `caps` list (the [`crate::msg::Msg::Hello`] field)
/// against the v1 host capability surface. Non-`host.*` entries — commands
/// the plugin serves, e.g. `command:feed` — are opaque and pass through
/// unvalidated; any `host.*` entry not in the v1 set is rejected. Returns the
/// declared host ops in order, so the host can check requirements without
/// re-parsing strings.
pub fn validate_caps(caps: &[String]) -> Result<Vec<HostCap>, CapsError> {
    let mut host_ops = Vec::new();
    for cap in caps {
        if cap.starts_with("host.") {
            host_ops.push(HostCap::parse(cap).ok_or_else(|| CapsError { op: cap.clone() })?);
        }
    }
    Ok(host_ops)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse ────────────────────────────────────────────────────────────────

    #[test]
    fn every_v1_op_parses() {
        for cap in ALL_CAPS {
            assert_eq!(
                HostCap::parse(cap.as_str()),
                Some(*cap),
                "op `{}`",
                cap.as_str()
            );
        }
    }

    #[test]
    fn all_caps_is_exactly_the_v1_surface() {
        assert_eq!(ALL_CAPS.len(), 15);
        let mut seen = std::collections::HashSet::new();
        for cap in ALL_CAPS {
            assert!(seen.insert(*cap), "duplicate op in ALL_CAPS");
        }
    }

    #[test]
    fn unknown_host_op_does_not_parse() {
        assert_eq!(HostCap::parse("host.frobnicate"), None);
        assert_eq!(HostCap::parse("host."), None);
        assert_eq!(HostCap::parse("host"), None);
    }

    #[test]
    fn non_host_cap_does_not_parse() {
        assert_eq!(HostCap::parse("command:feed"), None);
        assert_eq!(HostCap::parse(""), None);
    }

    // ── serde ────────────────────────────────────────────────────────────────

    #[test]
    fn each_op_serializes_to_its_wire_string() {
        for cap in ALL_CAPS {
            let wire = cap.as_str();
            let json = serde_json::to_string(cap).unwrap();
            assert_eq!(json, format!(r#""{wire}""#), "op `{wire}`");
        }
    }

    #[test]
    fn each_op_deserializes_from_its_wire_string() {
        for cap in ALL_CAPS {
            let wire = cap.as_str();
            let json = format!(r#""{wire}""#);
            assert_eq!(
                serde_json::from_str::<HostCap>(&json).unwrap(),
                *cap,
                "op `{wire}`"
            );
        }
    }

    #[test]
    fn unknown_wire_string_fails_to_deserialize() {
        assert!(serde_json::from_str::<HostCap>(r#""host.frobnicate""#).is_err());
    }

    // ── caps validation ──────────────────────────────────────────────────────

    #[test]
    fn mixed_caps_validate_and_return_declared_host_ops() {
        let caps = vec![
            "command:feed.list".into(),
            "host.kv.get".into(),
            "host.kv.set".into(),
            "host.open_view".into(),
        ];
        assert_eq!(
            validate_caps(&caps),
            Ok(vec![HostCap::KvGet, HostCap::KvSet, HostCap::OpenView])
        );
    }

    #[test]
    fn hello_style_caps_validate() {
        let hello = crate::msg::Msg::Hello {
            v: crate::msg::API_VERSION,
            name: "feed".into(),
            caps: vec!["command:feed".into(), "host.send_message".into()],
            manifest: None,
        };
        let crate::msg::Msg::Hello { caps, .. } = hello else {
            unreachable!()
        };
        assert_eq!(validate_caps(&caps), Ok(vec![HostCap::SendMessage]));
    }

    #[test]
    fn unknown_host_op_is_rejected() {
        let caps = vec!["command:feed".into(), "host.frobnicate".into()];
        assert_eq!(
            validate_caps(&caps),
            Err(CapsError {
                op: "host.frobnicate".into()
            })
        );
    }

    #[test]
    fn empty_caps_validate_to_empty() {
        assert_eq!(validate_caps(&[]), Ok(vec![]));
    }

    #[test]
    fn non_host_caps_alone_validate_to_empty() {
        let caps = vec!["command:feed".into(), "command:feed.subscribe".into()];
        assert_eq!(validate_caps(&caps), Ok(vec![]));
    }
}
