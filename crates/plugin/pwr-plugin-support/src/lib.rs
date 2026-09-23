//! Shared plugin-side plumbing for pwr-bot panel plugins.
//!
//! A panel plugin owns its view, update logic, and model vocabulary; the
//! mechanical plumbing every panel would otherwise copy — session state,
//! pending host-call bookkeeping, and wire writing — lives here once. The
//! crate is generic over the panel through [`Panel`]: a plugin implements it
//! for its model and gets [`SessionState`], [`Pending`], and
//! [`issue_host_call`] pre-wired to its service RPC pair (ADR-0010).
//!
//! Known fork: welcome duplicates [`SessionState`], [`Pending`],
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

pub use pwr_plugin_protocol::ABOUT_TARGET;
use pwr_plugin_protocol::Msg;
pub use pwr_plugin_protocol::SETTINGS_TARGET;
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
    /// The settings-persist RPC ([`Panel::UPDATE_SETTINGS_OP`]) a Back or
    /// About press issued. On its resp the panel hands the message to the
    /// host page it asked for through [`Pending::OpenSettings`]; without a
    /// captured [`ReturnExit`] there is nothing to return to, so the panel
    /// re-renders and stays.
    Persist {
        invoke_id: u64,
        session: SessionState<P>,
        exit: Option<ReturnExit>,
    },
    /// The `host.open_view` a [`ReturnExit`] issued against a
    /// host-reserved target — Back and About exits both ride it. Its resp
    /// answers with the wire's `ViewMoved` marker when the open replaced
    /// the panel's own message (the host page took it over, so re-rendering
    /// would overwrite it), or with the panel's own render when nothing
    /// took over (no live Settings session, or no source message to morph).
    OpenSettings {
        invoke_id: u64,
        session: SessionState<P>,
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
            Pending::OpenSettings { .. } => "host.open_view",
            Pending::Expire => P::UPDATE_SETTINGS_OP,
        }
    }
}

/// The `host.open_view` args that hand `message_id` (when the panel knows
/// the message its interaction fired on) back to the host Settings GUI,
/// carrying the guild the panel edits.
pub fn open_settings_args(channel_id: u64, guild_id: u64, message_id: Option<u64>) -> Value {
    return_args(SETTINGS_TARGET, channel_id, guild_id, message_id)
}

/// The `host.open_view` args that hand `message_id` to the host page the
/// host-reserved `target` names, carrying the guild the panel edits. The
/// `message_id` key is absent when the panel knows no source message: the
/// host then opens the page on a fresh message.
pub fn return_args(target: &str, channel_id: u64, guild_id: u64, message_id: Option<u64>) -> Value {
    let mut args = json!({
        "channel_id": channel_id,
        "plugin": target,
        "args": { "guild_id": guild_id },
    });
    if let Some(message_id) = message_id {
        args["message_id"] = json!(message_id);
    }
    args
}

/// The exit a panel press captured: the `host.open_view` args that hand the
/// panel's message to a host page (the reserved `settings` or `about`
/// target), and the source message id the open rides (it decides the
/// in-place `ViewMoved` answer).
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnExit {
    /// The `host.open_view` args against a host-reserved target.
    pub args: Value,
    /// The message the panel's interaction fired on, when known.
    pub message_id: Option<u64>,
}

/// Reads the Back exit off a `view.interact` args payload: the triggering
/// interaction's channel id and source message id become the in-place
/// `host.open_view` against the host-reserved `settings` target. `None`
/// when the interaction carries no channel id — there is nothing to return
/// in place, and the caller keeps the panel on screen.
pub fn back_exit(args: Option<&Value>, guild_id: u64) -> Option<ReturnExit> {
    return_exit(SETTINGS_TARGET, args, guild_id)
}

/// Reads the About exit off a `view.interact` args payload, mirroring
/// [`back_exit`] against the host-reserved `about` target: the host About
/// view opens on the panel's message. `None` when the interaction carries
/// no channel id — there is nothing to open in place, and the caller keeps
/// the panel on screen.
pub fn about_exit(args: Option<&Value>, guild_id: u64) -> Option<ReturnExit> {
    return_exit(ABOUT_TARGET, args, guild_id)
}

/// Builds the exit to a host-reserved target from a `view.interact` args
/// payload.
fn return_exit(target: &str, args: Option<&Value>, guild_id: u64) -> Option<ReturnExit> {
    let channel_id = args.and_then(|a| a.get("channel_id")).and_then(id_as_u64)?;
    let message_id = source_message_id(args);
    Some(ReturnExit {
        args: return_args(target, channel_id, guild_id, message_id),
        message_id,
    })
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

    #[test]
    fn open_settings_args_target_the_host_reserved_settings_and_edit_in_place() {
        assert_eq!(
            open_settings_args(5, 42, Some(777)),
            json!({
                "channel_id": 5,
                "plugin": SETTINGS_TARGET,
                "args": { "guild_id": 42 },
                "message_id": 777,
            })
        );
        assert!(
            open_settings_args(5, 42, None).get("message_id").is_none(),
            "no source message leaves the key absent, not null"
        );
    }

    #[test]
    fn back_exit_reads_the_channel_and_source_message_off_the_interaction() {
        let args = json!({
            "channel_id": "5",
            "message": { "id": "555" },
        });
        let back = back_exit(Some(&args), 42).expect("channel id present");
        assert_eq!(
            back.args,
            json!({
                "channel_id": 5,
                "plugin": SETTINGS_TARGET,
                "args": { "guild_id": 42 },
                "message_id": 555,
            })
        );
        assert_eq!(back.message_id, Some(555));
    }

    #[test]
    fn back_exit_without_a_source_message_omits_the_message_id() {
        let back = back_exit(Some(&json!({ "channel_id": "5" })), 42).expect("channel id present");

        assert_eq!(back.message_id, None);
        assert!(back.args.get("message_id").is_none());
    }

    #[test]
    fn back_exit_without_a_channel_id_is_none() {
        assert_eq!(back_exit(None, 42), None);
        assert_eq!(back_exit(Some(&json!({})), 42), None);
        assert_eq!(
            back_exit(Some(&json!({ "message": { "id": "555" } })), 42),
            None,
            "a modal submit without a channel id has nothing to return in place"
        );
    }

    #[test]
    fn about_exit_reads_the_channel_and_source_message_off_the_interaction() {
        let args = json!({
            "channel_id": "5",
            "message": { "id": "555" },
        });
        let about = about_exit(Some(&args), 42).expect("channel id present");
        assert_eq!(
            about.args,
            json!({
                "channel_id": 5,
                "plugin": ABOUT_TARGET,
                "args": { "guild_id": 42 },
                "message_id": 555,
            })
        );
        assert_eq!(about.message_id, Some(555));
    }

    #[test]
    fn about_exit_without_a_source_message_omits_the_message_id() {
        let about =
            about_exit(Some(&json!({ "channel_id": "5" })), 42).expect("channel id present");

        assert_eq!(about.message_id, None);
        assert!(about.args.get("message_id").is_none());
    }

    #[test]
    fn about_exit_without_a_channel_id_is_none() {
        assert_eq!(about_exit(None, 42), None);
        assert_eq!(about_exit(Some(&json!({})), 42), None);
        assert_eq!(
            about_exit(Some(&json!({ "message": { "id": "555" } })), 42),
            None,
            "a modal submit without a channel id has nothing to return in place"
        );
    }
}
