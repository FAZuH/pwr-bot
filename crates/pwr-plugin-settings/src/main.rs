//! The settings core plugin: the first real core plugin and the end-to-end
//! proof of the plugin infrastructure.
//!
//! Speaks the pwr-bot plugin wire protocol over JSON-Lines stdio, like the
//! `hello_plugin` fixture: one compact JSON object per line on stdout,
//! terminated by a single `\n` and flushed after every write; stderr is the
//! free logging channel.
//!
//! Behavior:
//! - announces `hello` (`v`, `name`, `caps`) as its first line after spawn;
//! - answers `invoke` of the `settings` command with the settings entry view,
//!   loading the persisted model from `host.kv.get` (`namespace='settings'`)
//!   on first open and applying the default model when the key is unset;
//! - answers `view.interact` (`settings:feeds` / `settings:voice` /
//!   `settings:welcome`) by toggling the model via the settings update logic
//!   and persisting it through `host.kv.set` before replying;
//! - every view reply is the full envelope `{"data", "ephemeral", "view"}`
//!   the interaction engine renders verbatim;
//! - treats `event` (e.g. `view.timeout`) as one-way, never answering it;
//! - answers `ping` with `pong`, tolerates the host's hello ack silently,
//!   and exits 0 on `bye` and on EOF.

use std::collections::HashMap;
use std::io::BufRead;
use std::io::Write;
use std::process::ExitCode;

use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::CommandDef;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::WireError;
use pwr_poise_components as components;
use serde_json::Value;
use serde_json::json;

/// The plugin's name: the command it serves and the handle the host keeps it
/// under.
const PLUGIN_NAME: &str = "settings";

/// The KV namespace the settings model is persisted under.
const KV_NAMESPACE: &str = "settings";

/// The KV key the settings model is persisted under.
const KV_MODEL_KEY: &str = "model";

/// Custom ids for the three feature toggles on the settings view.
const CUSTOM_ID_FEEDS: &str = "settings:feeds";
const CUSTOM_ID_VOICE: &str = "settings:voice";
const CUSTOM_ID_WELCOME: &str = "settings:welcome";

/// A plugin→host call in flight: the invoke id the reply must answer, and
/// what to do with the host's resp once it arrives.
#[derive(Debug, Clone, Copy)]
enum Pending {
    /// The `host.kv.get` issued to load the model before the first render.
    Load(u64),
    /// The `host.kv.set` issued to persist the model after a mutation.
    Save(u64),
}

// ── settings update logic (moved from the host's `src/update/settings_main.rs`) ──

/// Messages that mutate the settings model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsMsg {
    ToggleFeeds,
    ToggleVoice,
    ToggleWelcome,
}

/// The settings model: one enable flag per feature plus a dirty marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SettingsModel {
    feeds_enabled: bool,
    voice_enabled: bool,
    welcome_enabled: bool,
}

impl SettingsModel {
    /// The model as the JSON value persisted in KV.
    fn to_value(&self) -> Value {
        json!({
            "feeds": self.feeds_enabled,
            "voice": self.voice_enabled,
            "welcome": self.welcome_enabled,
        })
    }

    /// Parses a persisted model value; a missing or malformed value yields the
    /// default model.
    fn from_value(value: &Value) -> Self {
        Self {
            feeds_enabled: value.get("feeds").and_then(Value::as_bool).unwrap_or(false),
            voice_enabled: value.get("voice").and_then(Value::as_bool).unwrap_or(false),
            welcome_enabled: value
                .get("welcome")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }
}

impl Default for SettingsModel {
    /// A fresh model with every feature disabled.
    fn default() -> Self {
        Self {
            feeds_enabled: false,
            voice_enabled: false,
            welcome_enabled: false,
        }
    }
}

/// Applies a message to the model; every toggle marks it modified.
fn update(msg: SettingsMsg, model: &mut SettingsModel) {
    use SettingsMsg::*;
    match msg {
        ToggleFeeds => model.feeds_enabled = !model.feeds_enabled,
        ToggleVoice => model.voice_enabled = !model.voice_enabled,
        ToggleWelcome => model.welcome_enabled = !model.welcome_enabled,
    }
}

// ── view rendering ───────────────────────────────────────────────────────────

/// Renders the settings entry view: one toggle button per feature, with the
/// current state in the label and button style (green when enabled).
fn view_data(model: &SettingsModel) -> Value {
    let feeds = toggle_button(CUSTOM_ID_FEEDS, "Feeds", model.feeds_enabled);
    let voice = toggle_button(CUSTOM_ID_VOICE, "Voice", model.voice_enabled);
    let welcome = toggle_button(CUSTOM_ID_WELCOME, "Welcome", model.welcome_enabled);
    components::view_data(
        "-# **Settings**",
        [components::action_row([feeds, voice, welcome])],
    )
}

/// One feature toggle button: the label shows the state and the style follows
/// it (style 3 success when enabled, style 2 secondary when disabled).
fn toggle_button(custom_id: &str, label: &str, enabled: bool) -> Value {
    let state = if enabled { "✅" } else { "⬜" };
    let style = if enabled { 3 } else { 2 };
    components::button_with_style(custom_id, format!("{state} {label}"), style)
}

/// The full envelope a view reply carries: raw message data, visibility, and
/// the opaque view state the host stores and hands back on interactions.
fn envelope(model: &SettingsModel) -> Value {
    json!({
        "data": view_data(model),
        "ephemeral": false,
        "view": model.to_value(),
    })
}

// ── protocol helpers ─────────────────────────────────────────────────────────

/// The fixture's static declaration, matching what its hello announces.
fn manifest() -> Manifest {
    Manifest {
        name: PLUGIN_NAME.into(),
        description: "Manage server settings".into(),
        version: "0.1.0".into(),
        commands: vec![CommandDef {
            create_command: json!({"name": PLUGIN_NAME, "description": "Manage server settings"}),
        }],
        event_handlers: vec!["view.timeout".into()],
        tasks: vec![],
        api_version: API_VERSION,
    }
}

/// The `host.kv.get` call args for the settings model.
fn kv_get_args() -> Value {
    json!({ "namespace": KV_NAMESPACE, "key": KV_MODEL_KEY })
}

/// The `host.kv.set` call args persisting the settings model.
fn kv_set_args(model: &SettingsModel) -> Value {
    json!({
        "namespace": KV_NAMESPACE,
        "key": KV_MODEL_KEY,
        "value": serde_json::to_string(&model.to_value()).expect("serialize settings model"),
    })
}

/// Serializes `msg` to one JSON line, writes it, then flushes. Every protocol
/// line must end with `\n` and be flushed before the host can read it — piped
/// stdout is block-buffered.
fn write_msg(out: &mut impl Write, msg: &Msg) -> std::io::Result<()> {
    let line = serde_json::to_string(msg).expect("serialize protocol message");
    writeln!(out, "{line}")?;
    out.flush()
}

fn main() -> ExitCode {
    if let Err(e) = manifest().validate() {
        eprintln!("manifest invalid: {e}");
        return ExitCode::FAILURE;
    }

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut model: Option<SettingsModel> = None;
    let mut next_call_id: u64 = 0;
    // plugin->host calls in flight: our call id -> the invoke id to answer
    // once the host's resp arrives.
    let mut pending: HashMap<u64, Pending> = HashMap::new();

    // Announce ourselves: the plugin, not the host, sends hello first.
    let hello = Msg::Hello {
        v: API_VERSION,
        name: PLUGIN_NAME.into(),
        caps: vec![
            "command:settings".into(),
            "host.kv.get".into(),
            "host.kv.set".into(),
            "host.open_view".into(),
        ],
        manifest: Some(manifest()),
    };
    if write_msg(&mut out, &hello).is_err() {
        return ExitCode::FAILURE;
    }

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break }; // EOF => clean exit
        let msg: Msg = match serde_json::from_str(&line) {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("bad json: {e}");
                continue;
            }
        };
        match msg {
            Msg::Bye => break,
            Msg::Call { id, op, cmd, args } => {
                // A plugin->host call already in flight means this invoke must
                // wait for its resp; issue the next host call and keep the
                // chain going.
                // An invoke with the model already loaded answers immediately
                // with the entry view; the first invoke loads from KV first.
                if (op.as_str(), cmd.as_deref()) == ("invoke", Some(PLUGIN_NAME)) && model.is_some()
                {
                    let resp = Msg::resp_ok(id, Some(envelope(&model.unwrap_or_default())));
                    if write_msg(&mut out, &resp).is_err() {
                        return ExitCode::FAILURE;
                    }
                    continue;
                }
                let host_op = match (op.as_str(), cmd.as_deref()) {
                    ("invoke", Some(PLUGIN_NAME)) => Some(("host.kv.get", Pending::Load(id))),
                    ("view.interact", Some(PLUGIN_NAME)) => {
                        let custom_id = args
                            .as_ref()
                            .and_then(|a| a.get("custom_id"))
                            .and_then(Value::as_str);
                        let msg = match custom_id {
                            Some(CUSTOM_ID_FEEDS) => Some(SettingsMsg::ToggleFeeds),
                            Some(CUSTOM_ID_VOICE) => Some(SettingsMsg::ToggleVoice),
                            Some(CUSTOM_ID_WELCOME) => Some(SettingsMsg::ToggleWelcome),
                            _ => None,
                        };
                        let Some(msg) = msg else {
                            let resp = Msg::resp_err(
                                id,
                                WireError {
                                    kind: "UnknownAction".into(),
                                    msg: format!("unknown custom_id: {custom_id:?}"),
                                },
                            );
                            if write_msg(&mut out, &resp).is_err() {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
                        let mut current = model.unwrap_or_default();
                        update(msg, &mut current);
                        model = Some(current);
                        Some(("host.kv.set", Pending::Save(id)))
                    }
                    _ => None,
                };
                let Some((host_op, pending_kind)) = host_op else {
                    let cmd_repr = cmd.as_deref().unwrap_or("");
                    let resp = Msg::resp_err(
                        id,
                        WireError {
                            kind: "UnknownOp".into(),
                            msg: format!("unknown op {op} for cmd {cmd_repr}"),
                        },
                    );
                    if write_msg(&mut out, &resp).is_err() {
                        return ExitCode::FAILURE;
                    }
                    continue;
                };
                next_call_id += 1;
                pending.insert(next_call_id, pending_kind);
                let host_args = match host_op {
                    "host.kv.get" => kv_get_args(),
                    _ => kv_set_args(&model.unwrap_or_default()),
                };
                let host_call = Msg::Call {
                    id: next_call_id,
                    op: host_op.into(),
                    cmd: None,
                    args: Some(host_args),
                };
                if write_msg(&mut out, &host_call).is_err() {
                    return ExitCode::FAILURE;
                }
            }
            Msg::Event { name, .. } => {
                if name == "view.timeout" {
                    eprintln!("event: view.timeout received");
                }
            }
            Msg::Ping => {
                if write_msg(&mut out, &Msg::Pong).is_err() {
                    return ExitCode::FAILURE;
                }
            }
            Msg::Pong => {}
            // The host answers our hello with its own; tolerate it silently.
            Msg::Hello { .. } => {}
            Msg::Resp {
                id,
                ok,
                data,
                error,
            } => {
                let Some(pending_kind) = pending.remove(&id) else {
                    eprintln!("unexpected message: {line}");
                    continue;
                };
                match pending_kind {
                    Pending::Load(invoke_id) => {
                        // The stored model, or the default when unset or
                        // failed; then register the panel state in KV before
                        // the first render.
                        let loaded = if ok {
                            data.as_ref()
                                .and_then(|d| d.get("value"))
                                .and_then(Value::as_str)
                                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                                .map(|v| SettingsModel::from_value(&v))
                        } else {
                            None
                        };
                        let current = loaded.unwrap_or_default();
                        model = Some(current);
                        next_call_id += 1;
                        pending.insert(next_call_id, Pending::Save(invoke_id));
                        let host_call = Msg::Call {
                            id: next_call_id,
                            op: "host.kv.set".into(),
                            cmd: None,
                            args: Some(kv_set_args(&current)),
                        };
                        if write_msg(&mut out, &host_call).is_err() {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::Save(invoke_id) => {
                        if !ok {
                            eprintln!(
                                "host.kv.set failed: {:?}",
                                error.unwrap_or_else(|| WireError {
                                    kind: "HostError".into(),
                                    msg: "host call failed".into(),
                                })
                            );
                        }
                        let resp =
                            Msg::resp_ok(invoke_id, Some(envelope(&model.unwrap_or_default())));
                        if write_msg(&mut out, &resp).is_err() {
                            return ExitCode::FAILURE;
                        }
                    }
                }
            }
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_disables_every_feature() {
        let model = SettingsModel::default();
        assert!(!model.feeds_enabled);
        assert!(!model.voice_enabled);
        assert!(!model.welcome_enabled);
    }

    #[test]
    fn toggle_feeds_flips_feeds() {
        let mut model = SettingsModel::default();
        update(SettingsMsg::ToggleFeeds, &mut model);
        assert!(model.feeds_enabled);
        assert!(!model.voice_enabled);
        update(SettingsMsg::ToggleFeeds, &mut model);
        assert!(!model.feeds_enabled);
    }

    #[test]
    fn toggle_voice_flips_voice() {
        let mut model = SettingsModel::default();
        update(SettingsMsg::ToggleVoice, &mut model);
        assert!(model.voice_enabled);
    }

    #[test]
    fn toggle_welcome_flips_welcome() {
        let mut model = SettingsModel::default();
        update(SettingsMsg::ToggleWelcome, &mut model);
        assert!(model.welcome_enabled);
    }

    #[test]
    fn multiple_toggles_are_independent() {
        let mut model = SettingsModel::default();
        update(SettingsMsg::ToggleFeeds, &mut model);
        update(SettingsMsg::ToggleWelcome, &mut model);
        assert!(model.feeds_enabled);
        assert!(!model.voice_enabled);
        assert!(model.welcome_enabled);
    }

    #[test]
    fn model_round_trips_through_value() {
        let mut model = SettingsModel::default();
        update(SettingsMsg::ToggleFeeds, &mut model);
        update(SettingsMsg::ToggleVoice, &mut model);
        let parsed = SettingsModel::from_value(&model.to_value());
        assert_eq!(parsed, model);
    }

    #[test]
    fn malformed_value_falls_back_to_default() {
        let parsed = SettingsModel::from_value(&json!({"feeds": "nope"}));
        assert_eq!(parsed, SettingsModel::default());
    }

    #[test]
    fn envelope_carries_data_ephemeral_and_view() {
        let envelope = envelope(&SettingsModel::default());
        assert!(envelope.get("data").is_some());
        assert_eq!(envelope["ephemeral"], false);
        assert!(envelope.get("view").is_some());
    }

    #[test]
    fn view_data_renders_one_toggle_per_feature() {
        let data = view_data(&SettingsModel::default());
        assert_eq!(data["content"], "-# **Settings**");
        let buttons = &data["components"][0]["components"];
        assert_eq!(buttons.as_array().unwrap().len(), 3);
        assert_eq!(buttons[0]["custom_id"], CUSTOM_ID_FEEDS);
        assert_eq!(buttons[1]["custom_id"], CUSTOM_ID_VOICE);
        assert_eq!(buttons[2]["custom_id"], CUSTOM_ID_WELCOME);
    }

    #[test]
    fn enabled_feature_uses_the_success_style() {
        let model = SettingsModel {
            feeds_enabled: true,
            voice_enabled: false,
            welcome_enabled: false,
        };
        let buttons = &view_data(&model)["components"][0]["components"];
        assert_eq!(buttons[0]["style"], 3);
        assert_eq!(buttons[1]["style"], 2);
    }

    #[test]
    fn kv_set_args_serialize_the_model_as_a_string() {
        let model = SettingsModel {
            feeds_enabled: true,
            voice_enabled: false,
            welcome_enabled: false,
        };
        let args = kv_set_args(&model);
        assert_eq!(args["namespace"], KV_NAMESPACE);
        assert_eq!(args["key"], KV_MODEL_KEY);
        let parsed: Value = serde_json::from_str(args["value"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["feeds"], true);
        assert_eq!(parsed["voice"], false);
    }
}
