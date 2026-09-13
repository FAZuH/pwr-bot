//! Shared plugin-side plumbing for pwr-bot panel plugins (ADR-0009).
//!
//! A panel plugin owns its view, update logic, and model vocabulary; the
//! mechanical plumbing every panel would otherwise copy — session state,
//! pending host-call bookkeeping, wire writing, and hub navigation — lives
//! here once. The crate is generic over the panel through [`Panel`]: a
//! plugin implements it for its model and gets [`SessionState`],
//! [`Pending`], and [`issue_host_call`] pre-wired to its service RPC pair
//! (ADR-0010).
//!
//! Known fork: welcome-settings duplicates [`SessionState`], [`Pending`],
//! [`HostCall`], and [`issue_host_call`] locally. The shared session echo
//! carries the panel's whole model, but a welcome modal submission must
//! re-read settings rather than persist a stale snapshot, and the shared
//! [`Pending`] has no modal-reply arm.
//!
//! This crate speaks only the wire protocol's plugin side. It never
//! depends on the host crate, serenity, or poise: plugin crates keep zero
//! bot-crate dependencies, and the host keeps its own copy of the id
//! parsing twin.

use std::collections::HashMap;
use std::io::Write;

use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::ServerSettings;
use pwr_plugin_protocol::WireError;
use serde_json::Value;
use serde_json::json;

/// The per-panel vocabulary the plumbing is generic over: the model a
/// session carries plus the service RPC pair that loads and persists it.
/// A panel plugin implements this for its model type.
pub trait Panel {
    /// The `host.<feature>.get_settings` op that loads the snapshot.
    const GET_SETTINGS_OP: &'static str;
    /// The `host.<feature>.update_settings` op that persists the snapshot.
    const UPDATE_SETTINGS_OP: &'static str;
    /// Builds the model from the guild's whole settings snapshot.
    fn from_settings(settings: ServerSettings) -> Self;
    /// The guild's whole settings snapshot the model edits.
    fn settings(&self) -> &ServerSettings;
}

/// One view session's state: the guild the panel edits plus the model. It
/// rides the envelope's opaque `view` payload, which the host stores per
/// message and echoes back on every interaction and on `view.timeout`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionState<P> {
    /// The guild whose settings the session edits.
    pub guild_id: u64,
    /// The panel's model over the guild's snapshot.
    pub model: P,
}

impl<P: Panel> SessionState<P> {
    pub fn new(guild_id: u64, settings: ServerSettings) -> Self {
        Self {
            guild_id,
            model: P::from_settings(settings),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({
            "guild_id": self.guild_id,
            "settings": self.model.settings(),
        })
    }

    /// Parses a host-echoed `view` value; `None` on a missing or malformed
    /// payload.
    pub fn from_value(value: Option<&Value>) -> Option<Self> {
        let value = value?;
        let guild_id = value.get("guild_id").and_then(id_as_u64)?;
        let settings =
            serde_json::from_value(value.get("settings").cloned().unwrap_or(Value::Null)).ok()?;
        Some(Self {
            guild_id,
            model: P::from_settings(settings),
        })
    }
}

/// A plugin→host call in flight: the invoke id the reply must answer (when
/// an interaction started the chain), and what to do once the host's resp
/// arrives.
#[derive(Debug, Clone, PartialEq)]
pub enum Pending<P> {
    /// The settings-load RPC ([`Panel::GET_SETTINGS_OP`]) issued to load
    /// the model before the first render.
    LoadSettings { invoke_id: u64, guild_id: u64 },
    /// The settings-persist RPC ([`Panel::UPDATE_SETTINGS_OP`]) a terminal
    /// exit issued before returning to the hub. The channel the source
    /// interaction came from and the hub page to open follow the persist.
    Persist {
        invoke_id: u64,
        session: SessionState<P>,
        channel_id: Option<u64>,
        /// The message the interaction fired on: `open_hub_args` edits it in
        /// place instead of posting a fresh hub message.
        message_id: Option<u64>,
        hub_page: HubPage,
    },
    /// The `host.open_view` a completed persist issued for the hub.
    OpenHub {
        invoke_id: u64,
        session: SessionState<P>,
        /// The in-place marker: a successful open replaced the source
        /// message, so its resp answers `VIEW_MOVED_KIND` instead of the
        /// panel's own render.
        message_id: Option<u64>,
    },
    /// The settings-persist RPC an expiry issued: nothing to answer, the
    /// resp is only logged.
    Expire,
}

impl<P: Panel> Pending<P> {
    /// The host op this pending kind belongs to.
    pub fn op(&self) -> &'static str {
        match self {
            Pending::LoadSettings { .. } => P::GET_SETTINGS_OP,
            Pending::Persist { .. } => P::UPDATE_SETTINGS_OP,
            Pending::OpenHub { .. } => "host.open_view",
            Pending::Expire => P::UPDATE_SETTINGS_OP,
        }
    }
}

/// A plugin→host call: the pending kind its resp will resolve, and the
/// call's args.
pub struct HostCall<P> {
    pending: Pending<P>,
    args: Value,
}

impl<P: Panel> HostCall<P> {
    pub fn new(pending: Pending<P>, args: Value) -> Self {
        Self { pending, args }
    }
}

/// Which hub page the panel's About exit opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubPage {
    Hub,
    About,
}

impl HubPage {
    pub fn name(self) -> &'static str {
        match self {
            HubPage::Hub => "hub",
            HubPage::About => "about",
        }
    }
}

/// The settings hub's plugin name, the `host.open_view` target for Back.
pub const HUB_PLUGIN: &str = "settings";

/// Reads a Discord id from a wire value: a number, or the string form
/// serenity's ids serialize to.
pub fn id_as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

/// The source message the interaction fired on: serenity serializes
/// component and modal interactions with the source message under
/// `message`, and its id as a string. `None` when the payload carries no
/// source message, so the caller opens the view on a fresh message.
pub fn source_message_id(args: Option<&Value>) -> Option<u64> {
    args?.get("message")?.get("id").and_then(id_as_u64)
}

/// The `host.open_view` call args opening the settings hub on the given
/// page: the panel's Back lands where the monolith's
/// `Navigation::SettingsMain` did, and its About where
/// `Navigation::SettingsAbout` did. When the interaction carried a source
/// message id, the hub replaces that message instead of posting a fresh one.
pub fn open_hub_args(
    channel_id: u64,
    guild_id: u64,
    page: HubPage,
    message_id: Option<u64>,
) -> Value {
    let mut args = json!({
        "channel_id": channel_id,
        "plugin": HUB_PLUGIN,
        "command": HUB_PLUGIN,
        "args": { "guild_id": guild_id, "page": page.name() },
    });
    if let Some(message_id) = message_id {
        args["message_id"] = json!(message_id);
    }
    args
}

/// Serializes `msg` to one JSON line, writes it, then flushes.
pub fn write_msg(out: &mut impl Write, msg: &Msg) -> std::io::Result<()> {
    let line = serde_json::to_string(msg).expect("serialize protocol message");
    writeln!(out, "{line}")?;
    out.flush()
}

/// Writes a `resp_err` answering `invoke_id` with the given error kind and
/// message; returns whether the write succeeded.
pub fn reply_err(out: &mut impl Write, invoke_id: u64, kind: &str, msg: impl Into<String>) -> bool {
    let resp = Msg::resp_err(
        invoke_id,
        WireError {
            kind: kind.into(),
            msg: msg.into(),
        },
    );
    write_msg(out, &resp).is_ok()
}

/// Issues a plugin→host call: assigns the next call id, records the pending
/// kind its resp will resolve, and writes the `Msg::Call` line. Returns
/// whether the write succeeded.
pub fn issue_host_call<P: Panel>(
    out: &mut impl Write,
    pending: &mut HashMap<u64, Pending<P>>,
    next_call_id: &mut u64,
    call: HostCall<P>,
) -> bool {
    *next_call_id += 1;
    let HostCall {
        pending: pending_kind,
        args,
    } = call;
    let op = pending_kind.op();
    pending.insert(*next_call_id, pending_kind);
    let call_msg = Msg::Call {
        id: *next_call_id,
        op: op.into(),
        cmd: None,
        args: Some(args),
    };
    write_msg(out, &call_msg).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_as_u64_accepts_numbers_and_strings() {
        assert_eq!(id_as_u64(&json!(42)), Some(42));
        assert_eq!(id_as_u64(&json!("42")), Some(42));
        assert_eq!(id_as_u64(&json!("nope")), None);
        assert_eq!(id_as_u64(&json!(null)), None);
    }

    #[test]
    fn open_hub_args_carry_channel_guild_and_page() {
        assert_eq!(
            open_hub_args(5, 42, HubPage::About, None),
            json!({
                "channel_id": 5,
                "plugin": "settings",
                "command": "settings",
                "args": { "guild_id": 42, "page": "about" },
            })
        );
    }

    #[test]
    fn open_hub_args_edit_the_source_message_when_present() {
        assert_eq!(
            open_hub_args(5, 42, HubPage::Hub, Some(777)),
            json!({
                "channel_id": 5,
                "plugin": "settings",
                "command": "settings",
                "args": { "guild_id": 42, "page": "hub" },
                "message_id": 777,
            })
        );
    }

    #[test]
    fn source_message_id_reads_the_serenity_interaction_shape() {
        assert_eq!(
            source_message_id(Some(&json!({ "message": { "id": "555" } }))),
            Some(555),
            "serenity serializes ids as strings"
        );
        assert_eq!(
            source_message_id(Some(&json!({ "message": { "id": 555 } }))),
            Some(555)
        );
        assert_eq!(source_message_id(Some(&json!({ "channel_id": "9" }))), None);
        assert_eq!(
            source_message_id(Some(&json!({ "message": Value::Null }))),
            None,
            "a modal submit without a message falls back to a fresh message"
        );
        assert_eq!(source_message_id(None), None);
    }
}
