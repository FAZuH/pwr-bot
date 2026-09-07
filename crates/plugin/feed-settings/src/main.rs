//! The feed settings panel plugin (#148): per-guild feed subscription
//! settings served as a plugin view — the panel-migration program's tracer
//! bullet for typed service RPCs (ADR-0010).
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
//! - answers `invoke` of `feed-settings` by loading the guild's whole
//!   [`ServerSettings`] snapshot through `host.feed.get_settings` and
//!   rendering the monolith `/feed settings` panel as Components V2;
//! - answers `view.interact` by applying the monolith update vocabulary
//!   (toggle, channel, subscribe role, unsubscribe role) to the session's
//!   own model copy and re-rendering — a plain edit makes no host call;
//! - the terminal exits — `Back`, `About`, and the engine's `view.timeout`
//!   event — persist the session's snapshot exactly once through
//!   `host.feed.update_settings` (the monolith's save-on-exit semantics);
//! - `Back` then returns to the settings hub and `About` opens the hub's
//!   About page, both through `host.open_view` (the hub seeds its session
//!   page from the invoke args); after either, the panel's message keeps
//!   answering its own interactions — the hub opens next to it, like every
//!   plugin→plugin navigation;
//! - treats `event` (`view.timeout`) as one-way, never answering it: the
//!   persist it triggers rides a `host.feed.update_settings` call whose
//!   resp is only logged;
//! - answers `ping` with `pong`, tolerates the host's hello ack silently,
//!   and exits 0 on `bye` and on EOF.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::BufRead;
use std::io::Write;
use std::process::ExitCode;

use pwr_ext::view;
use pwr_ext::view_support::ButtonStyle;
use pwr_ext::view_support::ChannelType;
use pwr_ext::view_support::CreateSelectMenuKind;
use pwr_ext::view_support::GenericChannelId;
use pwr_ext::view_support::RoleId;
use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::ServerSettings;
use pwr_plugin_protocol::WireError;
use pwr_plugin_support::HubPage;
use pwr_plugin_support::Panel;
use pwr_plugin_support::id_as_u64;
use pwr_plugin_support::issue_host_call;
use pwr_plugin_support::open_hub_args;
use pwr_plugin_support::reply_err;
use pwr_plugin_support::write_msg;
use serde_json::Value;
use serde_json::json;

/// The plugin's name: the hello `name`, the hub's `host.open_view` target,
/// and the handle the host keeps it under.
const PLUGIN_NAME: &str = "feed-settings";

/// Custom ids for the panel's interactive components.
const CUSTOM_ID_TOGGLE: &str = "feeds:toggle";
const CUSTOM_ID_CHANNEL: &str = "feeds:channel";
const CUSTOM_ID_SUB_ROLE: &str = "feeds:sub-role";
const CUSTOM_ID_UNSUB_ROLE: &str = "feeds:unsub-role";
const CUSTOM_ID_BACK: &str = "feeds:back";
const CUSTOM_ID_ABOUT: &str = "feeds:about";

/// Panel copy, verbatim from the monolith's feed settings view.
const CHANNEL_TEXT: &str =
    "### Notification Channel\n\n> 🛈  Choose where feed updates will be posted.";
const SUB_ROLE_TEXT: &str = "### Subscribe Permission\n\n> 🛈  Who can add new feeds to this server. Leave empty to allow users with \"Manage Server\" permission.";
const UNSUB_ROLE_TEXT: &str = "### Unsubscribe Permission\n\n> 🛈  Who can remove feeds from this server. Leave empty to allow users with \"Manage Server\" permission.";

// ── the plugin's own update logic (ported from src/update/feed_settings.rs) ───────

/// The feed settings model: the guild's whole [`ServerSettings`] snapshot,
/// as the monolith's `FeedSettingsModel` held it.
#[derive(Debug, Clone, PartialEq)]
struct Model {
    settings: ServerSettings,
}

impl Model {
    fn new(settings: ServerSettings) -> Self {
        Self { settings }
    }

    /// Whether feed notifications are enabled (defaults to enabled).
    fn is_enabled(&self) -> bool {
        self.settings.feeds.enabled.unwrap_or(true)
    }

    fn channel_id(&self) -> Option<String> {
        self.settings.feeds.channel_id.clone()
    }

    fn subscribe_role_id(&self) -> Option<String> {
        self.settings.feeds.subscribe_role_id.clone()
    }

    fn unsubscribe_role_id(&self) -> Option<String> {
        self.settings.feeds.unsubscribe_role_id.clone()
    }
}

/// Messages that drive the model: the monolith `FeedSettingsMsg` vocabulary
/// (its lifecycle arm collapses to [`PanelMsg::Expired`], the only lifecycle
/// moment a plugin receives).
#[derive(Debug, Clone, PartialEq, Eq)]
enum PanelMsg {
    ToggleEnabled,
    SetChannel(Option<String>),
    SetSubRole(Option<String>),
    SetUnsubRole(Option<String>),
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
            let current = model.settings.feeds.enabled.unwrap_or(true);
            model.settings.feeds.enabled = Some(!current);
            Vec::new()
        }
        PanelMsg::SetChannel(id) => {
            model.settings.feeds.channel_id = id;
            Vec::new()
        }
        PanelMsg::SetSubRole(id) => {
            model.settings.feeds.subscribe_role_id = id;
            Vec::new()
        }
        PanelMsg::SetUnsubRole(id) => {
            model.settings.feeds.unsubscribe_role_id = id;
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
    const GET_SETTINGS_OP: &'static str = "host.feed.get_settings";
    const UPDATE_SETTINGS_OP: &'static str = "host.feed.update_settings";

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
/// `/feed settings` view: the status header (whose copy reflects the
/// enabled state and the configured channel), the toggle button, the
/// notification-channel select, the two permission-role selects, and the
/// Back/About row outside the container. The select kinds are built at
/// runtime so the current selection rides each menu's default values.
fn view_data(model: &Model) -> Value {
    let is_enabled = model.is_enabled();

    let status_text = format!(
        "-# **Settings > Feeds**\n## Feed Subscription Settings\n\n> 🛈  {}",
        if is_enabled {
            match model.channel_id() {
                Some(id) => format!(
                    "Feed notifications are currently **active**. Notifications will be sent to <#{id}>"
                ),
                None => "Feed notifications are currently **active**, but notification channel is not set.".to_string(),
            }
        } else {
            "Feed notifications are currently **paused**. No notifications will be sent until it is re-enabled.".to_string()
        }
    );

    let enabled_label = if is_enabled { "Disable" } else { "Enable" };
    let enabled_style = if is_enabled {
        ButtonStyle::Danger
    } else {
        ButtonStyle::Success
    };

    let channel_kind = CreateSelectMenuKind::Channel {
        channel_types: Some(Cow::Owned(vec![ChannelType::Text, ChannelType::News])),
        default_channels: model
            .channel_id()
            .and_then(|id| id.parse::<u64>().ok())
            .map(|id| Cow::Owned(vec![GenericChannelId::new(id)])),
    };
    let channel_placeholder = if model.channel_id().is_some() {
        "Change notification channel"
    } else {
        "⚠️ Required: Select a notification channel"
    };

    let sub_role_kind = CreateSelectMenuKind::Role {
        default_roles: model
            .subscribe_role_id()
            .and_then(|id| id.parse::<u64>().ok())
            .map(|id| Cow::Owned(vec![RoleId::new(id)])),
    };
    let sub_role_placeholder = if model.subscribe_role_id().is_some() {
        "Change subscribe role"
    } else {
        "Optional: Select role for subscribe permission"
    };

    let unsub_role_kind = CreateSelectMenuKind::Role {
        default_roles: model
            .unsubscribe_role_id()
            .and_then(|id| id.parse::<u64>().ok())
            .map(|id| Cow::Owned(vec![RoleId::new(id)])),
    };
    let unsub_role_placeholder = if model.unsubscribe_role_id().is_some() {
        "Change unsubscribe role"
    } else {
        "Optional: Select role for unsubscribe permission"
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
                text_display { content: CHANNEL_TEXT }
                action_row {
                    select_menu {
                        custom_id: CUSTOM_ID_CHANNEL,
                        kind: channel_kind,
                        placeholder: channel_placeholder
                    }
                }
                text_display { content: SUB_ROLE_TEXT }
                action_row {
                    select_menu {
                        custom_id: CUSTOM_ID_SUB_ROLE,
                        kind: sub_role_kind,
                        min_values: 0,
                        placeholder: sub_role_placeholder
                    }
                }
                text_display { content: UNSUB_ROLE_TEXT }
                action_row {
                    select_menu {
                        custom_id: CUSTOM_ID_UNSUB_ROLE,
                        kind: unsub_role_kind,
                        min_values: 0,
                        placeholder: unsub_role_placeholder
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
    serde_json::to_value(message).expect("feed settings view is serializable")
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
        description: "Manage feed subscription settings".into(),
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

/// The `host.feed.get_settings` call args.
fn get_settings_args(guild_id: u64) -> Value {
    json!({ "guild_id": guild_id })
}

/// The `host.feed.update_settings` call args persisting a snapshot.
fn update_settings_args(guild_id: u64, settings: &ServerSettings) -> Value {
    json!({ "guild_id": guild_id, "settings": settings })
}

/// Parses a `host.feed.get_settings` resp payload into a snapshot.
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
            "host.feed.get_settings".into(),
            "host.feed.update_settings".into(),
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
                        let values = args
                            .as_ref()
                            .and_then(|a| a.get("data"))
                            .and_then(|d| d.get("values"))
                            .and_then(Value::as_array);
                        let selected_id = || {
                            values
                                .and_then(|v| v.first())
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        };
                        let msg = match custom_id {
                            Some(CUSTOM_ID_TOGGLE) => PanelMsg::ToggleEnabled,
                            Some(CUSTOM_ID_CHANNEL) => PanelMsg::SetChannel(selected_id()),
                            Some(CUSTOM_ID_SUB_ROLE) => PanelMsg::SetSubRole(selected_id()),
                            Some(CUSTOM_ID_UNSUB_ROLE) => PanelMsg::SetUnsubRole(selected_id()),
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
                            eprintln!("host.feed.get_settings failed: {error:?}");
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
                        // settings to edit, so the hub's Feeds button shows
                        // the error it forwarded.
                        let Some(settings) = data.as_ref().and_then(parse_settings) else {
                            eprintln!("host.feed.get_settings resp carried no snapshot");
                            if !reply_err(
                                &mut out,
                                invoke_id,
                                "HostError",
                                "host.feed.get_settings resp carried no snapshot",
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
                        hub_page,
                    } => {
                        // The persist left the session: a failure logs but
                        // does not strand the user in a panel that thinks
                        // it is closed — the hub still opens.
                        if !ok {
                            eprintln!("host.feed.update_settings failed: {error:?}");
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
                            Pending::OpenHub { invoke_id, session },
                            open_hub_args(channel_id, guild_id, hub_page),
                        );
                        if !issue_host_call(&mut out, &mut pending, &mut next_call_id, call) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::OpenHub { invoke_id, session } => {
                        if !ok {
                            eprintln!("host.open_view failed: {error:?}");
                        }
                        // The panel keeps answering its own interactions;
                        // the hub opens next to it, like every
                        // plugin→plugin navigation.
                        if !reply_envelope(&mut out, invoke_id, &session) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::Expire => {
                        if !ok {
                            eprintln!("host.feed.update_settings failed on expiry: {error:?}");
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
            feeds: pwr_plugin_protocol::FeedsSettings {
                enabled: Some(true),
                channel_id: Some("123456789".into()),
                subscribe_role_id: Some("987654321".into()),
                unsubscribe_role_id: Some("987654322".into()),
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
    fn set_channel_updates_and_clears() {
        let mut m = model();
        update(PanelMsg::SetChannel(Some("42".into())), &mut m);
        assert_eq!(m.channel_id(), Some("42".to_string()));
        update(PanelMsg::SetChannel(None), &mut m);
        assert_eq!(m.channel_id(), None);
    }

    #[test]
    fn set_sub_and_unsub_roles_update_independently() {
        let mut m = model();
        update(PanelMsg::SetSubRole(Some("role1".into())), &mut m);
        update(PanelMsg::SetUnsubRole(Some("role2".into())), &mut m);
        assert_eq!(m.subscribe_role_id(), Some("role1".to_string()));
        assert_eq!(m.unsubscribe_role_id(), Some("role2".to_string()));
    }

    #[test]
    fn terminal_exits_persist_the_snapshot_exactly_once() {
        for msg in [PanelMsg::Back, PanelMsg::About, PanelMsg::Expired] {
            let mut m = model();
            let effects = update(msg.clone(), &mut m);
            assert_eq!(effects.len(), 1, "{msg:?}");
            match &effects[0] {
                Effect::Persist(s) => assert_eq!(s.feeds.channel_id.as_deref(), Some("123456789")),
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
        assert_eq!(
            children.len(),
            8,
            "status, toggle, channel, sub role, unsub role"
        );
        assert_eq!(
            children[0]["content"],
            json!(
                "-# **Settings > Feeds**\n## Feed Subscription Settings\n\n> 🛈  Feed notifications are currently **active**. Notifications will be sent to <#123456789>"
            )
        );

        let toggle = &children[1]["components"][0];
        assert_eq!(toggle["custom_id"], json!(CUSTOM_ID_TOGGLE));
        assert_eq!(toggle["label"], json!("Disable"));
        assert_eq!(toggle["style"], json!(4), "danger while enabled");

        let channel = &children[3]["components"][0];
        assert_eq!(channel["custom_id"], json!(CUSTOM_ID_CHANNEL));
        assert_eq!(channel["channel_types"], json!([0, 5]), "text and news");
        assert_eq!(
            channel["default_values"],
            json!([{ "id": 123456789, "type": "channel" }])
        );

        let sub = &children[5]["components"][0];
        assert_eq!(sub["custom_id"], json!(CUSTOM_ID_SUB_ROLE));
        assert_eq!(sub["min_values"], json!(0));
        assert_eq!(
            sub["default_values"],
            json!([{ "id": 987654321, "type": "role" }])
        );

        let unsub = &children[7]["components"][0];
        assert_eq!(unsub["custom_id"], json!(CUSTOM_ID_UNSUB_ROLE));
        assert_eq!(
            unsub["default_values"],
            json!([{ "id": 987654322, "type": "role" }])
        );

        let nav = components[1]["components"].as_array().expect("nav");
        assert_eq!(nav[0]["custom_id"], json!(CUSTOM_ID_BACK));
        assert_eq!(nav[0]["label"], json!("❮ Back"));
        assert_eq!(nav[1]["custom_id"], json!(CUSTOM_ID_ABOUT));
        assert_eq!(nav[1]["label"], json!("🛈 About"));
    }

    #[test]
    fn paused_panel_renders_the_paused_copy_and_enable_button() {
        let settings = ServerSettings {
            feeds: pwr_plugin_protocol::FeedsSettings {
                enabled: Some(false),
                ..Default::default()
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
        assert!(
            children[3]["components"][0]["placeholder"]
                .as_str()
                .unwrap()
                .contains("Required"),
            "no channel selected: the placeholder says so"
        );
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
            "host.feed.get_settings"
        );
        assert_eq!(
            Pending::Persist {
                invoke_id: 0,
                session: session.clone(),
                channel_id: None,
                hub_page: HubPage::Hub,
            }
            .op(),
            "host.feed.update_settings"
        );
        assert_eq!(
            Pending::OpenHub {
                invoke_id: 0,
                session
            }
            .op(),
            "host.open_view"
        );
        assert_eq!(Pending::Expire.op(), "host.feed.update_settings");
    }
}
