//! The voice settings panel plugin (#149): per-guild voice tracking settings
//! served as a plugin view — the panel-migration program's second panel,
//! replicating the feed-settings tracer (ADR-0009/0010).
//!
//! Speaks the pwr-bot plugin wire protocol over JSON-Lines stdio, like the
//! `settings` hub plugin: one compact JSON object per line on stdout,
//! terminated by a single `\n` and flushed after every write; stderr is the
//! free logging channel.
//!
//! Behavior:
//! - announces `hello` (`v`, `name`, `caps`) as its first line after spawn;
//!   the plugin is a core plugin with no slash command — it opens through
//!   the settings hub's `host.open_view` (ADR-0009), which forwards the
//!   source interaction's `guild_id` in the invoke args;
//! - answers `invoke` of `voice-settings` by loading the guild's whole
//!   [`ServerSettings`] snapshot through `host.voice.get_settings` and
//!   rendering the monolith `/vc settings` panel as Components V2;
//! - answers `view.interact` by applying the monolith update vocabulary
//!   (toggle) to the session's own model copy and re-rendering — a plain
//!   edit makes no host call;
//! - the terminal exits — `Back`, `About`, and the engine's `view.timeout`
//!   event — persist the session's snapshot exactly once through
//!   `host.voice.update_settings` (the monolith's save-on-exit semantics);
//! - `Back` then returns to the settings hub and `About` opens the hub's
//!   About page, both through `host.open_view` (the hub seeds its session
//!   page from the invoke args); after either, the panel's message keeps
//!   answering its own interactions — the hub opens next to it, like every
//!   plugin→plugin navigation;
//! - treats `event` (`view.timeout`) as one-way, never answering it: the
//!   persist it triggers rides a `host.voice.update_settings` call whose
//!   resp is only logged;
//! - answers `ping` with `pong`, tolerates the host's hello ack silently,
//!   and exits 0 on `bye` and on EOF.

use std::collections::HashMap;
use std::io::BufRead;
use std::io::Write;
use std::process::ExitCode;

use pwr_ext::view;
use pwr_ext::view_support::ButtonStyle;
use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::ServerSettings;
use pwr_plugin_protocol::VIEW_MOVED_KIND;
use pwr_plugin_protocol::WireError;
use pwr_plugin_support::HubPage;
use pwr_plugin_support::Panel;
use pwr_plugin_support::id_as_u64;
use pwr_plugin_support::issue_host_call;
use pwr_plugin_support::open_hub_args;
use pwr_plugin_support::reply_err;
use pwr_plugin_support::source_message_id;
use pwr_plugin_support::write_msg;
use serde_json::Value;
use serde_json::json;

/// The plugin's name: the hello `name`, the hub's `host.open_view` target,
/// and the handle the host keeps it under.
const PLUGIN_NAME: &str = "voice-settings";

/// Custom ids for the panel's interactive components.
const CUSTOM_ID_TOGGLE: &str = "voice:toggle";
const CUSTOM_ID_BACK: &str = "voice:back";
const CUSTOM_ID_ABOUT: &str = "voice:about";

// ── the plugin's own update logic (ported from src/update/voice_settings.rs) ─────

/// The voice settings model: the guild's whole [`ServerSettings`] snapshot,
/// as the monolith's `VoiceSettingsModel` held it.
#[derive(Debug, Clone, PartialEq)]
struct Model {
    settings: ServerSettings,
}

impl Model {
    fn new(settings: ServerSettings) -> Self {
        Self { settings }
    }

    /// Whether voice tracking is enabled (defaults to enabled).
    fn is_enabled(&self) -> bool {
        self.settings.voice.enabled.unwrap_or(true)
    }
}

/// Messages that drive the model: the monolith `VoiceSettingsMsg` vocabulary
/// (its lifecycle arm collapses to [`PanelMsg::Expired`], the only lifecycle
/// moment a plugin receives).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelMsg {
    ToggleEnabled,
    Back,
    About,
    Expired,
}

/// Effects the model can request: a persist of the whole snapshot, exactly
/// like the monolith's single `PersistSettings` effect.
#[derive(Debug, Clone, PartialEq)]
enum Effect {
    Persist(ServerSettings),
}

/// The pure update function — the only writer of the model. Terminal exits
/// (`Back`, `About`, `Expired`) persist the current snapshot exactly once;
/// every edit applies in place with no effect.
fn update(msg: PanelMsg, model: &mut Model) -> Vec<Effect> {
    match msg {
        PanelMsg::ToggleEnabled => {
            let current = model.settings.voice.enabled.unwrap_or(true);
            model.settings.voice.enabled = Some(!current);
            Vec::new()
        }
        PanelMsg::Back | PanelMsg::About | PanelMsg::Expired => persist(model),
    }
}

/// The persist-on-exit behavior shared by `Back`, `About`, and expiry:
/// snapshot the current settings exactly once.
fn persist(model: &Model) -> Vec<Effect> {
    vec![Effect::Persist(model.settings.clone())]
}

// ── shared plumbing (crates/plugin/pwr-plugin-support) ────────────────────────

/// How this panel plugs into the support crate's generic plumbing: the model
/// a session carries, and the service RPC pair that loads and persists it.
impl Panel for Model {
    const GET_SETTINGS_OP: &'static str = "host.voice.get_settings";
    const UPDATE_SETTINGS_OP: &'static str = "host.voice.update_settings";

    fn from_settings(settings: ServerSettings) -> Self {
        Model::new(settings)
    }

    fn settings(&self) -> &ServerSettings {
        &self.settings
    }
}

type SessionState = pwr_plugin_support::SessionState<Model>;
type Pending = pwr_plugin_support::Pending<Model>;
type HostCall = pwr_plugin_support::HostCall<Model>;

// ── view rendering ────────────────────────────────────────────────────────────

/// Renders the panel as Components V2, mirroring the monolith's
/// `/vc settings` view: the status header (whose copy reflects the enabled
/// state), the toggle button, and the Back/About row outside the container.
fn view_data(model: &Model) -> Value {
    let is_enabled = model.is_enabled();

    let status_text = format!(
        "-# **Settings > Voice**\n## Voice Tracking Settings\n\n> 🛈  {}",
        if is_enabled {
            "Voice tracking is **active**."
        } else {
            "Voice tracking is **paused**."
        }
    );

    let enabled_label = if is_enabled { "Disable" } else { "Enable" };
    let enabled_style = if is_enabled {
        ButtonStyle::Danger
    } else {
        ButtonStyle::Success
    };

    let message = view! {
        components_v2 {
            container {
                text_display { content: status_text }
                action_row {
                    button {
                        custom_id: CUSTOM_ID_TOGGLE,
                        label: enabled_label,
                        style: enabled_style
                    }
                }
            }
            action_row {
                button {
                    custom_id: CUSTOM_ID_BACK,
                    label: "❮ Back",
                    style: ButtonStyle::Secondary
                }
                button {
                    custom_id: CUSTOM_ID_ABOUT,
                    label: "🛈 About",
                    style: ButtonStyle::Secondary
                }
            }
        }
    };
    serde_json::to_value(message).expect("voice settings view is serializable")
}

/// The full envelope a view reply carries: raw message data, visibility,
/// and the session state the host stores per message.
fn envelope(session: &SessionState) -> Value {
    json!({
        "data": view_data(&session.model),
        "ephemeral": false,
        "view": session.to_value(),
    })
}

/// Writes an ok resp answering `invoke_id` with the session's envelope.
/// Returns whether the write succeeded.
fn reply_envelope(out: &mut impl Write, invoke_id: u64, session: &SessionState) -> bool {
    let resp = Msg::resp_ok(invoke_id, Some(envelope(session)));
    write_msg(out, &resp).is_ok()
}

// ── protocol helpers ───────────────────────────────────────────────────────────

/// The plugin's static declaration, matching what its hello announces.
fn manifest() -> Manifest {
    Manifest {
        name: PLUGIN_NAME.into(),
        description: "Manage voice tracking settings".into(),
        version: "0.1.0".into(),
        // No slash command: the panel opens through the settings hub
        // (ADR-0009). A direct slash invoke carries no `guild_id` in its
        // re-parsed args, so a command would open a panel with no guild to
        // edit.
        commands: vec![],
        event_handlers: vec!["view.timeout".into()],
        tasks: vec![],
        api_version: API_VERSION,
    }
}

/// The `host.voice.get_settings` call args.
fn get_settings_args(guild_id: u64) -> Value {
    json!({ "guild_id": guild_id })
}

/// The `host.voice.update_settings` call args persisting a snapshot.
fn update_settings_args(guild_id: u64, settings: &ServerSettings) -> Value {
    json!({ "guild_id": guild_id, "settings": settings })
}

/// Parses a `host.voice.get_settings` resp payload into a snapshot.
fn parse_settings(data: &Value) -> Option<ServerSettings> {
    serde_json::from_value(data.clone()).ok()
}

// ── the event loop ────────────────────────────────────────────────────────────

fn main() -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut next_call_id: u64 = 0;
    // plugin->host calls in flight: our call id -> the pending kind whose
    // resp completes this call chain.
    let mut pending: HashMap<u64, Pending> = HashMap::new();

    // Announce ourselves: the plugin, not the host, sends hello first.
    let hello = Msg::Hello {
        v: API_VERSION,
        name: PLUGIN_NAME.into(),
        caps: vec![
            "host.voice.get_settings".into(),
            "host.voice.update_settings".into(),
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
                // An invoke with the model loaded first renders the panel;
                // a plain edit re-renders without a host call; the terminal
                // exits persist, then re-open the hub.
                let host_call = match (op.as_str(), cmd.as_deref()) {
                    ("invoke", Some(PLUGIN_NAME)) => {
                        // The hub forwards the source interaction's guild
                        // id; the model loads before the first render.
                        let Some(guild_id) = args
                            .as_ref()
                            .and_then(|a| a.get("guild_id"))
                            .and_then(id_as_u64)
                        else {
                            if !reply_err(&mut out, id, "InvalidArgs", "missing `guild_id` (u64)") {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
                        Some(HostCall::new(
                            Pending::LoadSettings {
                                invoke_id: id,
                                guild_id,
                            },
                            get_settings_args(guild_id),
                        ))
                    }
                    ("view.interact", Some(PLUGIN_NAME)) => {
                        // The session state the host echoed back.
                        let Some(session) =
                            SessionState::from_value(args.as_ref().and_then(|a| a.get("view")))
                        else {
                            if !reply_err(
                                &mut out,
                                id,
                                "InvalidArgs",
                                "missing or malformed session `view` state",
                            ) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
                        let custom_id = args
                            .as_ref()
                            .and_then(|a| a.get("custom_id"))
                            .and_then(Value::as_str);
                        let msg = match custom_id {
                            Some(CUSTOM_ID_TOGGLE) => PanelMsg::ToggleEnabled,
                            Some(CUSTOM_ID_BACK) => PanelMsg::Back,
                            Some(CUSTOM_ID_ABOUT) => PanelMsg::About,
                            Some(other) => {
                                if !reply_err(
                                    &mut out,
                                    id,
                                    "UnknownAction",
                                    format!("unknown custom_id: {other}"),
                                ) {
                                    return ExitCode::FAILURE;
                                }
                                continue;
                            }
                            None => {
                                if !reply_err(&mut out, id, "InvalidArgs", "missing `custom_id`") {
                                    return ExitCode::FAILURE;
                                }
                                continue;
                            }
                        };
                        let hub_page = match custom_id {
                            Some(CUSTOM_ID_ABOUT) => HubPage::About,
                            _ => HubPage::Hub,
                        };
                        let channel_id = args
                            .as_ref()
                            .and_then(|a| a.get("channel_id"))
                            .and_then(id_as_u64);
                        // The message the click fired on: Back/About edit it
                        // in place instead of posting a fresh hub message.
                        let message_id = source_message_id(args.as_ref());
                        let mut session = session;
                        let effects = update(msg, &mut session.model);
                        if effects.is_empty() {
                            // A plain edit re-renders immediately.
                            if !reply_envelope(&mut out, id, &session) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        }
                        // A terminal exit persists first, then re-opens the
                        // hub: the channel the source interaction came from,
                        // and the hub page About asks for.
                        let persist_args =
                            update_settings_args(session.guild_id, &session.model.settings);
                        Some(HostCall::new(
                            Pending::Persist {
                                invoke_id: id,
                                session,
                                channel_id,
                                message_id,
                                hub_page,
                            },
                            persist_args,
                        ))
                    }
                    _ => None,
                };
                let Some(call) = host_call else {
                    let cmd_repr = cmd.as_deref().unwrap_or("");
                    if !reply_err(
                        &mut out,
                        id,
                        "UnknownOp",
                        format!("unknown op {op} for cmd {cmd_repr}"),
                    ) {
                        return ExitCode::FAILURE;
                    }
                    continue;
                };
                if !issue_host_call(&mut out, &mut pending, &mut next_call_id, call) {
                    return ExitCode::FAILURE;
                }
            }
            Msg::Event { name, data } => {
                if name != "view.timeout" {
                    continue;
                }
                // The expiry carries the session's last state: the terminal
                // message persists the snapshot exactly once, answering
                // nothing — events are one-way.
                let Some(session) = SessionState::from_value(data.as_ref()) else {
                    eprintln!("view.timeout without parseable session state");
                    continue;
                };
                let mut model = session.model.clone();
                let effects = update(PanelMsg::Expired, &mut model);
                let Effect::Persist(snapshot) = effects.first().cloned().unwrap_or_else(|| {
                    // Unreachable — `Expired` always persists — but a failed
                    // exit must never silently drop the save.
                    Effect::Persist(model.settings.clone())
                });
                let call = HostCall::new(
                    Pending::Expire,
                    update_settings_args(session.guild_id, &snapshot),
                );
                if !issue_host_call(&mut out, &mut pending, &mut next_call_id, call) {
                    return ExitCode::FAILURE;
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
                    Pending::LoadSettings {
                        invoke_id,
                        guild_id,
                    } => {
                        if !ok {
                            eprintln!("host.voice.get_settings failed: {error:?}");
                            let error = error.unwrap_or_else(|| WireError {
                                kind: "HostError".into(),
                                msg: "host call failed".into(),
                            });
                            if !reply_err(&mut out, invoke_id, &error.kind, error.msg) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        }
                        // A failed load fails the open: the panel has no
                        // settings to edit, so the hub's Voice button shows
                        // the error it forwarded.
                        let Some(settings) = data.as_ref().and_then(parse_settings) else {
                            eprintln!("host.voice.get_settings resp carried no snapshot");
                            if !reply_err(
                                &mut out,
                                invoke_id,
                                "HostError",
                                "host.voice.get_settings resp carried no snapshot",
                            ) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
                        if !reply_envelope(
                            &mut out,
                            invoke_id,
                            &SessionState::new(guild_id, settings),
                        ) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::Persist {
                        invoke_id,
                        session,
                        channel_id,
                        message_id,
                        hub_page,
                    } => {
                        // The persist left the session: a failure logs but
                        // does not strand the user in a panel that thinks
                        // it is closed — the hub still opens.
                        if !ok {
                            eprintln!("host.voice.update_settings failed: {error:?}");
                        }
                        let Some(channel_id) = channel_id else {
                            eprintln!("back/about without a channel id: panel stays");
                            if !reply_envelope(&mut out, invoke_id, &session) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
                        let guild_id = session.guild_id;
                        let call = HostCall::new(
                            Pending::OpenHub {
                                invoke_id,
                                session,
                                message_id,
                            },
                            open_hub_args(channel_id, guild_id, hub_page, message_id),
                        );
                        if !issue_host_call(&mut out, &mut pending, &mut next_call_id, call) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::OpenHub {
                        invoke_id,
                        session,
                        message_id,
                    } => {
                        if !ok {
                            eprintln!("host.open_view failed: {error:?}");
                        }
                        // In place: the open replaced this panel's message
                        // with the hub, so answering with the panel's own
                        // render would overwrite it — the host skips its
                        // render on the marker kind. Without a source
                        // message the hub opened next to the panel, which
                        // keeps answering its own interactions.
                        if ok && message_id.is_some() {
                            if !reply_err(&mut out, invoke_id, VIEW_MOVED_KIND, "hub opened") {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        }
                        if !reply_envelope(&mut out, invoke_id, &session) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::Expire => {
                        if !ok {
                            eprintln!("host.voice.update_settings failed on expiry: {error:?}");
                        }
                        // Nothing to answer: the event was one-way.
                    }
                }
            }
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use pwr_poise_components::IS_COMPONENTS_V2;

    use super::*;

    fn model() -> Model {
        let settings = ServerSettings {
            voice: pwr_plugin_protocol::VoiceSettings {
                enabled: Some(true),
            },
            ..ServerSettings::default()
        };
        Model::new(settings)
    }

    // ── update logic (mirrors the monolith module's tests) ─────────────────

    #[test]
    fn toggle_flips_enabled() {
        let mut m = model();
        let effects = update(PanelMsg::ToggleEnabled, &mut m);
        assert!(effects.is_empty());
        assert!(!m.is_enabled());
    }

    #[test]
    fn toggle_defaults_to_enabled() {
        let mut m = Model::new(ServerSettings::default());
        update(PanelMsg::ToggleEnabled, &mut m);
        assert!(!m.is_enabled());
    }

    #[test]
    fn terminal_exits_persist_the_snapshot_exactly_once() {
        for msg in [PanelMsg::Back, PanelMsg::About, PanelMsg::Expired] {
            let mut m = model();
            let effects = update(msg, &mut m);
            assert_eq!(effects.len(), 1, "{msg:?}");
            match &effects[0] {
                Effect::Persist(s) => assert_eq!(s.voice.enabled, Some(true)),
            }
        }
    }

    // ── session state ──────────────────────────────────────────────────────

    #[test]
    fn session_state_round_trips_through_value() {
        let session = SessionState::new(42, model().settings);
        let parsed = SessionState::from_value(Some(&session.to_value()));
        assert_eq!(parsed.as_ref(), Some(&session));
    }

    #[test]
    fn malformed_session_state_yields_none() {
        assert_eq!(SessionState::from_value(None), None);
        assert_eq!(SessionState::from_value(Some(&json!({}))), None);
        assert_eq!(
            SessionState::from_value(Some(&json!({"guild_id": 42}))),
            None
        );
        assert_eq!(
            SessionState::from_value(Some(&json!({"guild_id": "42", "settings": {}}))),
            Some(SessionState::new(42, ServerSettings::default())),
            "a string guild id parses, and sections default"
        );
    }

    // ── view rendering ──────────────────────────────────────────────────────

    #[test]
    fn panel_is_components_v2_without_legacy_content() {
        let data = view_data(&model());
        assert_eq!(data["flags"], json!(IS_COMPONENTS_V2));
        assert!(data.get("content").is_none(), "v2 carries no top content");
    }

    #[test]
    fn panel_mirrors_the_monolith_layout() {
        let data = view_data(&model());
        let components = data["components"].as_array().expect("components");
        assert_eq!(components.len(), 2, "container plus the nav row");

        let children = components[0]["components"].as_array().expect("children");
        assert_eq!(children.len(), 2, "status, toggle");
        assert_eq!(
            children[0]["content"],
            json!(
                "-# **Settings > Voice**\n## Voice Tracking Settings\n\n> 🛈  Voice tracking is **active**."
            )
        );

        let toggle = &children[1]["components"][0];
        assert_eq!(toggle["custom_id"], json!(CUSTOM_ID_TOGGLE));
        assert_eq!(toggle["label"], json!("Disable"));
        assert_eq!(toggle["style"], json!(4), "danger while enabled");

        let nav = components[1]["components"].as_array().expect("nav");
        assert_eq!(nav[0]["custom_id"], json!(CUSTOM_ID_BACK));
        assert_eq!(nav[0]["label"], json!("❮ Back"));
        assert_eq!(nav[1]["custom_id"], json!(CUSTOM_ID_ABOUT));
        assert_eq!(nav[1]["label"], json!("🛈 About"));
    }

    #[test]
    fn paused_panel_renders_the_paused_copy_and_enable_button() {
        let settings = ServerSettings {
            voice: pwr_plugin_protocol::VoiceSettings {
                enabled: Some(false),
            },
            ..ServerSettings::default()
        };
        let data = view_data(&Model::new(settings));
        let children = data["components"][0]["components"]
            .as_array()
            .expect("children");
        assert!(
            children[0]["content"]
                .as_str()
                .unwrap()
                .contains("**paused**")
        );
        assert_eq!(children[1]["components"][0]["label"], json!("Enable"));
        assert_eq!(children[1]["components"][0]["style"], json!(3), "success");
    }

    // ── protocol args ───────────────────────────────────────────────────────

    #[test]
    fn get_and_update_args_carry_the_guild_and_snapshot() {
        assert_eq!(get_settings_args(7), json!({ "guild_id": 7 }));
        let settings = model().settings;
        let args = update_settings_args(7, &settings);
        assert_eq!(args["guild_id"], json!(7));
        assert_eq!(
            serde_json::from_value::<ServerSettings>(args["settings"].clone()).unwrap(),
            settings
        );
    }

    #[test]
    fn parse_settings_accepts_a_snapshot_and_rejects_garbage() {
        let settings = model().settings;
        assert_eq!(
            parse_settings(&serde_json::to_value(&settings).unwrap()),
            Some(settings)
        );
        assert_eq!(parse_settings(&json!("nope")), None);
    }

    #[test]
    fn pending_ops_name_their_host_ops() {
        let session = SessionState::new(1, ServerSettings::default());
        assert_eq!(
            Pending::LoadSettings {
                invoke_id: 0,
                guild_id: 1
            }
            .op(),
            "host.voice.get_settings"
        );
        assert_eq!(
            Pending::Persist {
                invoke_id: 0,
                session: session.clone(),
                channel_id: None,
                message_id: None,
                hub_page: HubPage::Hub,
            }
            .op(),
            "host.voice.update_settings"
        );
        assert_eq!(
            Pending::OpenHub {
                invoke_id: 0,
                session,
                message_id: None,
            }
            .op(),
            "host.open_view"
        );
        assert_eq!(Pending::Expire.op(), "host.voice.update_settings");
    }
}
