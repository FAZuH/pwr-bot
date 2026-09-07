//! The welcome settings panel plugin (#151): per-guild welcome-card settings
//! served as a plugin view — the panel-migration program's third panel,
//! replicating the monolith `/welcome` panel (ADR-0009/0010) and the first
//! plugin to use the modal capability (ADR-0011).
//!
//! Speaks the pwr-bot plugin wire protocol over JSON-Lines stdio, like the
//! other panel plugins: one compact JSON object per line on stdout,
//! terminated by a single `\n` and flushed after every write; stderr is the
//! free logging channel.
//!
//! Behavior:
//! - announces `hello` (`v`, `name`, `caps`) as its first line after spawn;
//!   the plugin is a core plugin with no slash command — it opens through
//!   the settings hub's `host.open_view` (ADR-0009), which forwards the
//!   source interaction's `guild_id` in the invoke args;
//! - answers `invoke` of `welcome-settings` by loading the guild's whole
//!   [`ServerSettings`] snapshot through `host.welcome.get_settings` and
//!   rendering the monolith `/welcome` panel as Components V2;
//! - unlike the feed and voice panels, the session state it echoes carries
//!   only the guild id and the pending message-removal selection — never the
//!   settings. Every interaction re-reads the guild's snapshot through
//!   `host.welcome.get_settings` before it applies its message, so a modal
//!   submission (whose answer the host does not commit to the session) can
//!   never render or persist a stale snapshot;
//! - every mutating message persists immediately through
//!   `host.welcome.update_settings` (the monolith's persist-on-every-change
//!   semantics, no-op edits included); the terminal messages (`Back`,
//!   `About`) persist nothing and only re-open the hub;
//! - the `Add Welcome Message` and `Set Color` buttons open their modals
//!   through `host.open_modal` (ADR-0011) and answer the click with the
//!   [`MODAL_OPENED_KIND`] wire error, so the host skips its own response —
//!   the modal IS the response. The submission arrives later as a
//!   `view.modal_submit` call, identified by the nonce-suffixed modal
//!   custom id (unique per open, so an author-keyed route collision cannot
//!   mis-apply an answer); the removal selection rides a small in-process
//!   stash across the modal round trip;
//! - declares the preview attachment slot in every envelope (ADR-0012): the
//!   `attachments` field names [`WELCOME_FILE`] while welcome cards are
//!   enabled and is empty while they are off. The host renders the card and
//!   attaches the bytes at transport; the plugin never sees image bytes;
//! - answers `ping` with `pong`, tolerates the host's hello ack silently,
//!   and exits 0 on `bye` and on EOF. There is no `view.timeout` handler:
//!   the monolith's expiry persists nothing, so an expired panel has
//!   nothing left to do.

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::BufRead;
use std::io::Write;
use std::process::ExitCode;

use pwr_ext::component;
use pwr_ext::view;
use pwr_ext::view_support::ButtonStyle;
use pwr_ext::view_support::ChannelType;
use pwr_ext::view_support::CreateButton;
use pwr_ext::view_support::CreateContainerComponent;
use pwr_ext::view_support::CreateSelectMenuKind;
use pwr_ext::view_support::CreateSelectMenuOption;
use pwr_ext::view_support::GenericChannelId;
use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::MODAL_OPENED_KIND;
use pwr_plugin_protocol::MODAL_SUBMIT_OP;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::ServerSettings;
use pwr_plugin_protocol::WireError;
use pwr_plugin_support::HubPage;
use pwr_plugin_support::id_as_u64;
use pwr_plugin_support::open_hub_args;
use pwr_plugin_support::reply_err;
use pwr_plugin_support::write_msg;
use serde_json::Value;
use serde_json::json;

/// The plugin's name: the hello `name`, the hub's `host.open_view` target,
/// and the handle the host keeps it under.
const PLUGIN_NAME: &str = "welcome-settings";

/// Filename of the welcome preview attachment, matching the monolith's
/// `WELCOME_FILE`. The envelope declares this slot; the host fills it.
const WELCOME_FILE: &str = "welcome_preview.png";

/// Custom ids for the panel's interactive components.
const CUSTOM_ID_TOGGLE: &str = "welcome:toggle";
const CUSTOM_ID_CHANNEL: &str = "welcome:channel";
const CUSTOM_ID_TEMPLATE: &str = "welcome:template";
const CUSTOM_ID_COLOR: &str = "welcome:color";
const CUSTOM_ID_ADD: &str = "welcome:add";
const CUSTOM_ID_REMOVE: &str = "welcome:remove";
const CUSTOM_ID_SAVE: &str = "welcome:save";
const CUSTOM_ID_CANCEL: &str = "welcome:cancel";
const CUSTOM_ID_BACK: &str = "welcome:back";
const CUSTOM_ID_ABOUT: &str = "welcome:about";

/// Custom ids of the modals' text inputs, read back from the submission.
const INPUT_MESSAGE: &str = "message";
const INPUT_COLOR: &str = "color";

/// The link button's target, verbatim from the monolith.
const PREVIEW_TEMPLATES_URL: &str =
    "https://github.com/FAZuH/pwr-bot/blob/main/docs/welcome_templates_preview.png";

/// The template variables help text, verbatim from the monolith.
const VARIABLES_TEXT: &str = "### Template Variables\n> `{{ username }}` - User's display name\n> `{{ user_tag }}` - User's handle (@username)\n> `{{ server_name }}` - Server name\n> `{{ member_count }}` - Total member count\n> `{{ member_number }}` - Member join number\n> `{{ primary_color }}` - Accent color\n> `{{ welcome_message }}` - Your greetings";

/// The message cap, verbatim from the monolith's update logic.
const MAX_MESSAGES: usize = 25;

// ── the plugin's own update logic (ported from src/update/welcome_settings.rs) ────

/// The welcome settings model: the guild's whole [`ServerSettings`] snapshot
/// plus the pending removal selection, as the monolith's
/// `WelcomeSettingsModel` held them. The preview image bytes are absent by
/// design — the host renders and attaches them (ADR-0012).
#[derive(Debug, Clone, PartialEq)]
struct Model {
    settings: ServerSettings,
    marked_removal: HashSet<usize>,
}

impl Model {
    /// Whether welcome cards are enabled (defaults to disabled).
    fn is_enabled(&self) -> bool {
        self.settings.welcome.enabled.unwrap_or(false)
    }

    fn message_count(&self) -> usize {
        self.settings.welcome.messages.as_ref().map_or(0, Vec::len)
    }
}

/// Messages that drive the model: the monolith `WelcomeSettingsMsg`
/// vocabulary minus its lifecycle and async-result arms — a plugin persists
/// on every change, so expiry has nothing to save and the persist/render
/// results are the event loop's business, not the model's.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PanelMsg {
    ToggleEnabled,
    SetChannel(Option<String>),
    SetTemplate(Option<String>),
    MarkRemoval(HashSet<usize>),
    AddMessage(String),
    SetColor(String),
    SaveRemoval,
    CancelRemoval,
    Back,
    About,
}

/// Effects the model can request: a persist of the whole snapshot. The
/// monolith's paired `RenderImage` effect is host-side here — the envelope
/// declares the attachment slot and the host re-renders it on every
/// transport (ADR-0012).
#[derive(Debug, Clone, PartialEq)]
enum Effect {
    Persist(ServerSettings),
}

/// The pure update function — the only writer of the model. Every mutating
/// message persists, including no-op edits (empty or over-cap messages,
/// colors without a leading `#`), exactly as the monolith did. The terminal
/// messages (`Back`, `About`) and the selection-only messages persist
/// nothing.
fn update(msg: PanelMsg, model: &mut Model) -> Vec<Effect> {
    match msg {
        PanelMsg::Back | PanelMsg::About => Vec::new(),
        PanelMsg::ToggleEnabled => {
            let current = model.settings.welcome.enabled.unwrap_or(false);
            model.settings.welcome.enabled = Some(!current);
            persist(model)
        }
        PanelMsg::SetChannel(channel_id) => {
            model.settings.welcome.channel_id = channel_id;
            persist(model)
        }
        PanelMsg::SetTemplate(template_id) => {
            model.settings.welcome.template_id = template_id;
            persist(model)
        }
        PanelMsg::MarkRemoval(indices) => {
            model.marked_removal = indices;
            Vec::new()
        }
        PanelMsg::AddMessage(msg) => {
            let trimmed = msg.trim().to_string();
            if !trimmed.is_empty() && model.message_count() < MAX_MESSAGES {
                model
                    .settings
                    .welcome
                    .messages
                    .get_or_insert_with(Vec::new)
                    .push(trimmed);
            }
            persist(model)
        }
        PanelMsg::SetColor(color) => {
            let trimmed = color.trim().to_string();
            if trimmed.starts_with('#') {
                model.settings.welcome.primary_color = Some(trimmed);
            }
            persist(model)
        }
        PanelMsg::SaveRemoval => {
            let msgs = model.settings.welcome.messages.clone().unwrap_or_default();
            model.settings.welcome.messages = Some(
                msgs.into_iter()
                    .enumerate()
                    .filter(|(i, _)| !model.marked_removal.contains(i))
                    .map(|(_, msg)| msg)
                    .collect(),
            );
            model.marked_removal.clear();
            persist(model)
        }
        PanelMsg::CancelRemoval => {
            model.marked_removal.clear();
            Vec::new()
        }
    }
}

/// The persist-on-change behavior every mutating message shares: snapshot
/// the current settings exactly once.
fn persist(model: &Model) -> Vec<Effect> {
    vec![Effect::Persist(model.settings.clone())]
}

// ── session state ─────────────────────────────────────────────────────────────

/// One view session's echoed state: the guild the panel edits and the
/// pending removal selection. The settings deliberately do NOT ride along —
/// every interaction re-reads them, so a modal submission (which the host
/// does not commit) cannot resurrect a stale snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct SessionState {
    guild_id: u64,
    marked_removal: HashSet<usize>,
}

impl SessionState {
    fn new(guild_id: u64) -> Self {
        Self {
            guild_id,
            marked_removal: HashSet::new(),
        }
    }

    fn with_marked(mut self, marked: HashSet<usize>) -> Self {
        self.marked_removal = marked;
        self
    }

    fn to_value(&self) -> Value {
        let mut marked: Vec<usize> = self.marked_removal.iter().copied().collect();
        marked.sort_unstable();
        json!({
            "guild_id": self.guild_id,
            "marked_removal": marked,
        })
    }

    /// Parses a host-echoed `view` value; `None` on a missing or malformed
    /// payload. A missing `marked_removal` is an empty selection.
    fn from_value(value: Option<&Value>) -> Option<Self> {
        let value = value?;
        let guild_id = value.get("guild_id").and_then(id_as_u64)?;
        let marked = match value.get("marked_removal") {
            Some(Value::Array(entries)) => entries
                .iter()
                .filter_map(|entry| entry.as_u64().map(|i| i as usize))
                .collect(),
            Some(Value::Null) | None => HashSet::new(),
            Some(_) => return None,
        };
        Some(Self {
            guild_id,
            marked_removal: marked,
        })
    }
}

// ── plugin→host call bookkeeping ──────────────────────────────────────────────

/// A plugin→host call in flight: the invoke id the reply must answer, the
/// session state to echo, and what to do once the host's resp arrives. The
/// settings snapshot rides the variants that render (the load's resp is the
/// only copy the plugin holds); the ones that only chain carry the session.
#[derive(Debug, Clone, PartialEq)]
enum Pending {
    /// The load issued before the first render.
    LoadSettings { invoke_id: u64, guild_id: u64 },
    /// The load issued before applying a click.
    Interact {
        invoke_id: u64,
        session: SessionState,
        msg: PanelMsg,
        channel_id: Option<u64>,
        hub_page: HubPage,
    },
    /// The persist a mutating click or modal submission issued; its resp
    /// answers with the re-rendered panel.
    Persist {
        invoke_id: u64,
        session: SessionState,
        settings: ServerSettings,
    },
    /// The `host.open_view` a `Back`/`About` issued; its resp answers with
    /// the panel (the hub opens beside it, like every plugin→plugin
    /// navigation).
    OpenHub {
        invoke_id: u64,
        session: SessionState,
        settings: ServerSettings,
    },
    /// The `host.open_modal` a modal trigger issued; its resp answers the
    /// click with [`MODAL_OPENED_KIND`] so the host skips its own response.
    OpenModal { invoke_id: u64 },
    /// The load issued before applying a modal submission.
    ModalSubmit {
        invoke_id: u64,
        session: SessionState,
        msg: PanelMsg,
    },
}

impl Pending {
    /// The host op this pending kind belongs to.
    fn op(&self) -> &'static str {
        match self {
            Pending::LoadSettings { .. }
            | Pending::Interact { .. }
            | Pending::ModalSubmit { .. } => GET_SETTINGS_OP,
            Pending::Persist { .. } => UPDATE_SETTINGS_OP,
            Pending::OpenHub { .. } => "host.open_view",
            Pending::OpenModal { .. } => "host.open_modal",
        }
    }
}

/// A plugin→host call: the pending kind its resp will resolve, and the
/// call's args.
struct HostCall {
    pending: Pending,
    args: Value,
}

impl HostCall {
    fn new(pending: Pending, args: Value) -> Self {
        Self { pending, args }
    }
}

/// Issues a plugin→host call: assigns the next call id, records the pending
/// kind its resp will resolve, and writes the `Msg::Call` line. Returns
/// whether the write succeeded.
fn issue_host_call(
    out: &mut impl Write,
    pending: &mut HashMap<u64, Pending>,
    next_call_id: &mut u64,
    call: HostCall,
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

// ── view rendering ────────────────────────────────────────────────────────────

/// Truncates a removal option's label at a char boundary, as the monolith
/// did at byte offsets (which panic on multibyte text mid-label).
fn truncate(msg: &str, keep: usize) -> String {
    let mut out: String = msg.chars().take(keep).collect();
    out.push_str("...");
    out
}

/// The removal select's option label for one message: marked entries carry a
/// cross and the default flag, long messages are truncated.
fn removal_label(msg: &str, marked: bool) -> String {
    if marked {
        if msg.chars().count() > 48 {
            format!("❌ {}", truncate(msg, 45))
        } else {
            format!("❌ {msg}")
        }
    } else if msg.chars().count() > 50 {
        truncate(msg, 47)
    } else {
        msg.to_string()
    }
}

/// Renders the panel as Components V2, mirroring the monolith's `/welcome`
/// view: the status header, the toggle, the channel and template selects,
/// the Set Color / Add Welcome Message / Preview Templates row, the
/// variables help, the conditional removal select with its save/cancel row,
/// and the Back/About row outside the container.
fn view_data(model: &Model) -> Value {
    let is_enabled = model.is_enabled();
    let msgs = model.message_count();

    let status_text = format!(
        "-# **Settings > Welcome**\n## Welcome Settings\n\n> 🛈  {}",
        if is_enabled {
            "Welcome cards are **active**."
        } else {
            "Welcome cards are **disabled**."
        }
    );

    let enabled_label = if is_enabled { "Disable" } else { "Enable" };
    let enabled_style = if is_enabled {
        ButtonStyle::Danger
    } else {
        ButtonStyle::Success
    };

    let channel_kind = CreateSelectMenuKind::Channel {
        channel_types: Some(Cow::Owned(vec![ChannelType::Text])),
        default_channels: model
            .settings
            .welcome
            .channel_id
            .as_deref()
            .and_then(|id| id.parse::<u64>().ok())
            .map(|id| Cow::Owned(vec![GenericChannelId::new(id)])),
    };

    let templates: Vec<CreateSelectMenuOption<'static>> = (1..=12)
        .map(|i| CreateSelectMenuOption::new(format!("Template {i}"), i.to_string()))
        .collect();
    let template_kind = CreateSelectMenuKind::String {
        options: Cow::Owned(templates),
    };
    let template_placeholder = format!(
        "Select Template (Current: {})",
        model
            .settings
            .welcome
            .template_id
            .clone()
            .unwrap_or_else(|| "1".to_string())
    );

    let add_button: Option<CreateButton<'static>> = (msgs < MAX_MESSAGES).then(|| {
        CreateButton::new(CUSTOM_ID_ADD)
            .label("Add Welcome Message")
            .style(ButtonStyle::Primary)
    });

    let removal_select: Option<CreateContainerComponent<'static>> = (msgs > 0).then(|| {
        let options: Vec<CreateSelectMenuOption<'static>> = model
            .settings
            .welcome
            .messages
            .as_ref()
            .map(|messages| {
                messages
                    .iter()
                    .enumerate()
                    .map(|(i, msg)| {
                        let mut opt = CreateSelectMenuOption::new(
                            removal_label(msg, model.marked_removal.contains(&i)),
                            i.to_string(),
                        );
                        if model.marked_removal.contains(&i) {
                            opt = opt.default_selection(true);
                        }
                        opt
                    })
                    .collect()
            })
            .unwrap_or_default();
        let kind = CreateSelectMenuKind::String {
            options: Cow::Owned(options),
        };
        CreateContainerComponent::ActionRow(component! {
            action_row {
                select_menu {
                    custom_id: CUSTOM_ID_REMOVE,
                    kind: kind,
                    min_values: 0,
                    max_values: msgs as u8,
                    placeholder: "Select messages to remove"
                }
            }
        })
    });

    let removal_actions: Option<CreateContainerComponent<'static>> =
        (!model.marked_removal.is_empty()).then(|| {
            CreateContainerComponent::ActionRow(component! {
                action_row {
                    button {
                        custom_id: CUSTOM_ID_SAVE,
                        label: "Save Removals",
                        style: ButtonStyle::Danger
                    }
                    button {
                        custom_id: CUSTOM_ID_CANCEL,
                        label: "Cancel",
                        style: ButtonStyle::Secondary
                    }
                }
            })
        });

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
                action_row {
                    select_menu {
                        custom_id: CUSTOM_ID_CHANNEL,
                        kind: channel_kind,
                        placeholder: "Select Welcome Channel"
                    }
                }
                action_row {
                    select_menu {
                        custom_id: CUSTOM_ID_TEMPLATE,
                        kind: template_kind,
                        placeholder: template_placeholder
                    }
                }
                action_row {
                    button {
                        custom_id: CUSTOM_ID_COLOR,
                        label: "Set Color",
                        style: ButtonStyle::Primary
                    }
                    { add_button }
                    button {
                        url: PREVIEW_TEMPLATES_URL,
                        label: "Preview Templates"
                    }
                }
                text_display { content: VARIABLES_TEXT }
                { removal_select }
                { removal_actions }
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
    }
    .expect("spliced view obeys the component laws");
    serde_json::to_value(message).expect("welcome settings view is serializable")
}

/// The envelope's `attachments` declaration (ADR-0012): the preview slot by
/// filename while welcome cards are enabled, an explicit empty list while
/// they are off (which removes the attachment on edit). The host resolves
/// the slot into bytes at transport; the plugin never holds them.
fn attachment_declaration(model: &Model) -> Value {
    if model.is_enabled() {
        json!([{ "id": 0, "filename": WELCOME_FILE }])
    } else {
        json!([])
    }
}

/// The full envelope a view reply carries: raw message data with the
/// attachment declaration, visibility, and the session state the host
/// stores per message.
fn envelope(session: &SessionState, model: &Model) -> Value {
    let mut data = view_data(model);
    data["attachments"] = attachment_declaration(model);
    json!({
        "data": data,
        "ephemeral": false,
        "view": session.to_value(),
    })
}

/// Writes an ok resp answering `invoke_id` with the panel envelope built
/// from the loaded snapshot and the session's selection. Returns whether
/// the write succeeded.
fn reply_panel(
    out: &mut impl Write,
    invoke_id: u64,
    session: &SessionState,
    settings: &ServerSettings,
) -> bool {
    let model = Model {
        settings: settings.clone(),
        marked_removal: session.marked_removal.clone(),
    };
    let resp = Msg::resp_ok(invoke_id, Some(envelope(session, &model)));
    write_msg(out, &resp).is_ok()
}

// ── modals ────────────────────────────────────────────────────────────────────

/// Which modal a trigger button opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modal {
    AddMessage,
    SetColor,
}

impl Modal {
    /// The modal's custom id for one open: nonce-suffixed so every open is
    /// unique per process. The submission carries it back, which identifies
    /// the modal even when an author-keyed route collision replaces the
    /// binding, and keys the marked-removal stash.
    fn custom_id(self, nonce: u64) -> String {
        let prefix = match self {
            Modal::AddMessage => CUSTOM_ID_ADD,
            Modal::SetColor => CUSTOM_ID_COLOR,
        };
        format!("{prefix}:{nonce}")
    }

    /// The modal spec in the host's modal JSON grammar (the shape
    /// `host.open_modal` validates): a label-wrapped text input with the
    /// monolith's copy and limits.
    fn spec(self, custom_id: &str) -> Value {
        match self {
            Modal::AddMessage => json!({
                "custom_id": custom_id,
                "title": "Add Welcome Message",
                "components": [{
                    "type": 18,
                    "label": "Message",
                    "component": {
                        "type": 4,
                        "style": 2,
                        "custom_id": INPUT_MESSAGE,
                        "min_length": 1,
                        "max_length": 200,
                        "required": true,
                        "placeholder": "Welcome to {{ server_name }}, {{ user_tag }}!"
                    }
                }]
            }),
            Modal::SetColor => json!({
                "custom_id": custom_id,
                "title": "Set Primary Color",
                "components": [{
                    "type": 18,
                    "label": "Primary Color (Hex)",
                    "component": {
                        "type": 4,
                        "style": 1,
                        "custom_id": INPUT_COLOR,
                        "min_length": 4,
                        "max_length": 7,
                        "required": true,
                        "placeholder": "#5865F2"
                    }
                }]
            }),
        }
    }

    /// The modal a trigger custom id opens, if the id is one.
    fn from_trigger(custom_id: &str) -> Option<Self> {
        match custom_id {
            CUSTOM_ID_ADD => Some(Modal::AddMessage),
            CUSTOM_ID_COLOR => Some(Modal::SetColor),
            _ => None,
        }
    }

    /// The modal a submission carries, read from the submission's own
    /// `data.custom_id` (never the hoisted binding id, which names what the
    /// route last pointed at, not what was submitted).
    fn from_submission(custom_id: &str) -> Option<Self> {
        if custom_id.starts_with(&format!("{CUSTOM_ID_ADD}:")) {
            Some(Modal::AddMessage)
        } else if custom_id.starts_with(&format!("{CUSTOM_ID_COLOR}:")) {
            Some(Modal::SetColor)
        } else {
            None
        }
    }
}

/// The `host.open_modal` call args: the author of the triggering interaction
/// (the submission's routing key), the interaction id and token the modal
/// opens as a response to, and the modal spec.
fn open_modal_args(author_id: u64, interaction_id: u64, token: &str, modal: Value) -> Value {
    json!({
        "author_id": author_id,
        "interaction_id": interaction_id,
        "token": token,
        "modal": modal,
    })
}

/// Reads the triggering interaction's identity from the `view.interact`
/// args: the raw interaction is merged in, so `id`, `token`, and
/// `user.id` ride the top level.
fn interaction_identity(args: Option<&Value>) -> Option<(u64, String, u64)> {
    let args = args?;
    let interaction_id = args.get("id").and_then(id_as_u64)?;
    let token = args.get("token")?.as_str()?.to_string();
    let author_id = args.get("user")?.get("id").and_then(id_as_u64)?;
    Some((interaction_id, token, author_id))
}

/// Reads one text input's submitted value from a `view.modal_submit` args
/// payload: the raw submission's label-wrapped components.
fn modal_input_value(args: Option<&Value>, input: &str) -> Option<String> {
    let components = args?.get("data")?.get("components")?.as_array()?;
    components.iter().find_map(|entry| {
        let field = entry.get("component")?;
        if field.get("custom_id")?.as_str()? != input {
            return None;
        }
        Some(field.get("value")?.as_str()?.to_string())
    })
}

// ── protocol helpers ───────────────────────────────────────────────────────────

/// The settings RPC pair this panel's host ops belong to.
const GET_SETTINGS_OP: &str = "host.welcome.get_settings";
const UPDATE_SETTINGS_OP: &str = "host.welcome.update_settings";

/// The plugin's static declaration, matching what its hello announces.
fn manifest() -> Manifest {
    Manifest {
        name: PLUGIN_NAME.into(),
        description: "Manage welcome card settings".into(),
        version: "0.1.0".into(),
        // No slash command: the panel opens through the settings hub
        // (ADR-0009). A direct slash invoke carries no `guild_id` in its
        // re-parsed args, so a command would open a panel with no guild to
        // edit. No event handlers: the monolith's expiry persists nothing,
        // so there is no `view.timeout` work left for a plugin.
        commands: vec![],
        event_handlers: vec![],
        tasks: vec![],
        api_version: API_VERSION,
    }
}

/// The `host.welcome.get_settings` call args.
fn get_settings_args(guild_id: u64) -> Value {
    json!({ "guild_id": guild_id })
}

/// The `host.welcome.update_settings` call args persisting a snapshot.
fn update_settings_args(guild_id: u64, settings: &ServerSettings) -> Value {
    json!({ "guild_id": guild_id, "settings": settings })
}

/// Parses a `host.welcome.get_settings` resp payload into a snapshot.
fn parse_settings(data: &Value) -> Option<ServerSettings> {
    serde_json::from_value(data.clone()).ok()
}

/// The selected value of a single-value select interaction.
fn selected_id(args: Option<&Value>) -> Option<String> {
    args.and_then(|a| a.get("data"))
        .and_then(|d| d.get("values"))
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// The marked indices of a removal-select interaction: the selected values
/// are the option indices; non-numeric values are ignored, as the monolith
/// did.
fn selected_indices(args: Option<&Value>) -> HashSet<usize> {
    args.and_then(|a| a.get("data"))
        .and_then(|d| d.get("values"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .filter_map(|value| value.parse::<usize>().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Maps a click's custom id to its message, `None` for an unknown id. The
/// modal triggers are not here: they are handled before the load, as they
/// never mutate the model.
fn click_msg(custom_id: &str, args: Option<&Value>) -> Option<PanelMsg> {
    let msg = match custom_id {
        CUSTOM_ID_TOGGLE => PanelMsg::ToggleEnabled,
        CUSTOM_ID_CHANNEL => PanelMsg::SetChannel(selected_id(args)),
        CUSTOM_ID_TEMPLATE => PanelMsg::SetTemplate(selected_id(args)),
        CUSTOM_ID_REMOVE => PanelMsg::MarkRemoval(selected_indices(args)),
        CUSTOM_ID_SAVE => PanelMsg::SaveRemoval,
        CUSTOM_ID_CANCEL => PanelMsg::CancelRemoval,
        CUSTOM_ID_BACK => PanelMsg::Back,
        CUSTOM_ID_ABOUT => PanelMsg::About,
        _ => return None,
    };
    Some(msg)
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
    // Opened modals: the modal custom id -> the removal selection to carry
    // across the modal round trip. A dismissed modal never submits, so its
    // entry stays: a few bytes per dismissal in a per-process map, bounded
    // by the plugin's lifetime.
    let mut stash: HashMap<String, HashSet<usize>> = HashMap::new();
    let mut nonce: u64 = 0;

    // Announce ourselves: the plugin, not the host, sends hello first.
    let hello = Msg::Hello {
        v: API_VERSION,
        name: PLUGIN_NAME.into(),
        caps: vec![
            GET_SETTINGS_OP.into(),
            UPDATE_SETTINGS_OP.into(),
            "host.open_view".into(),
            "host.open_modal".into(),
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
                // Every interaction loads the guild's snapshot first: the
                // plugin holds no settings between calls. A modal trigger
                // instead opens its modal and answers the click with the
                // modal-opened marker.
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
                        let Some(custom_id) = custom_id else {
                            if !reply_err(&mut out, id, "InvalidArgs", "missing `custom_id`") {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
                        if let Some(modal) = Modal::from_trigger(custom_id) {
                            // A modal trigger: open the modal as the click's
                            // response and stash the selection for the
                            // submission. No settings load — the panel does
                            // not change.
                            let Some((interaction_id, token, author_id)) =
                                interaction_identity(args.as_ref())
                            else {
                                if !reply_err(
                                    &mut out,
                                    id,
                                    "InvalidArgs",
                                    "missing interaction `id`, `token`, or `user.id`",
                                ) {
                                    return ExitCode::FAILURE;
                                }
                                continue;
                            };
                            nonce += 1;
                            let modal_id = modal.custom_id(nonce);
                            let spec = modal.spec(&modal_id);
                            stash.insert(modal_id, session.marked_removal.clone());
                            Some(HostCall::new(
                                Pending::OpenModal { invoke_id: id },
                                open_modal_args(author_id, interaction_id, &token, spec),
                            ))
                        } else if let Some(msg) = click_msg(custom_id, args.as_ref()) {
                            let hub_page = if custom_id == CUSTOM_ID_ABOUT {
                                HubPage::About
                            } else {
                                HubPage::Hub
                            };
                            let channel_id = args
                                .as_ref()
                                .and_then(|a| a.get("channel_id"))
                                .and_then(id_as_u64);
                            let guild_id = session.guild_id;
                            Some(HostCall::new(
                                Pending::Interact {
                                    invoke_id: id,
                                    session,
                                    msg,
                                    channel_id,
                                    hub_page,
                                },
                                get_settings_args(guild_id),
                            ))
                        } else {
                            if !reply_err(
                                &mut out,
                                id,
                                "UnknownAction",
                                format!("unknown custom_id: {custom_id}"),
                            ) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        }
                    }
                    (MODAL_SUBMIT_OP, _) => {
                        // A submission of a modal this plugin opened. The
                        // modal identity is the submission's own custom id;
                        // the guild id rides the raw interaction, and the
                        // stashed selection carries over from the open.
                        let submitted = args
                            .as_ref()
                            .and_then(|a| a.get("data"))
                            .and_then(|d| d.get("custom_id"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let Some(modal) = Modal::from_submission(submitted) else {
                            if !reply_err(
                                &mut out,
                                id,
                                "UnknownAction",
                                format!("unknown modal submission: {submitted}"),
                            ) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
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
                        let input = match modal {
                            Modal::AddMessage => modal_input_value(args.as_ref(), INPUT_MESSAGE),
                            Modal::SetColor => modal_input_value(args.as_ref(), INPUT_COLOR),
                        };
                        let Some(input) = input else {
                            if !reply_err(
                                &mut out,
                                id,
                                "InvalidArgs",
                                "modal submission carried no input value",
                            ) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
                        let msg = match modal {
                            Modal::AddMessage => PanelMsg::AddMessage(input),
                            Modal::SetColor => PanelMsg::SetColor(input),
                        };
                        let marked = stash.remove(submitted).unwrap_or_default();
                        let session = SessionState::new(guild_id).with_marked(marked);
                        Some(HostCall::new(
                            Pending::ModalSubmit {
                                invoke_id: id,
                                session,
                                msg,
                            },
                            get_settings_args(guild_id),
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
            Msg::Event { .. } => {
                // No event handlers are declared, so nothing arrives worth
                // answering; events are one-way regardless.
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
                            eprintln!("{GET_SETTINGS_OP} failed: {error:?}");
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
                        // settings to edit.
                        let Some(settings) = data.as_ref().and_then(parse_settings) else {
                            eprintln!("{GET_SETTINGS_OP} resp carried no snapshot");
                            if !reply_err(
                                &mut out,
                                invoke_id,
                                "HostError",
                                "host.welcome.get_settings resp carried no snapshot",
                            ) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
                        if !reply_panel(
                            &mut out,
                            invoke_id,
                            &SessionState::new(guild_id),
                            &settings,
                        ) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::Interact {
                        invoke_id,
                        session,
                        msg,
                        channel_id,
                        hub_page,
                    } => {
                        if !ok {
                            eprintln!("{GET_SETTINGS_OP} failed: {error:?}");
                            let error = error.unwrap_or_else(|| WireError {
                                kind: "HostError".into(),
                                msg: "host call failed".into(),
                            });
                            if !reply_err(&mut out, invoke_id, &error.kind, error.msg) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        }
                        let Some(settings) = data.as_ref().and_then(parse_settings) else {
                            eprintln!("{GET_SETTINGS_OP} resp carried no snapshot");
                            if !reply_err(
                                &mut out,
                                invoke_id,
                                "HostError",
                                "host.welcome.get_settings resp carried no snapshot",
                            ) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
                        let mut model = Model {
                            settings,
                            marked_removal: session.marked_removal.clone(),
                        };
                        let effects = update(msg.clone(), &mut model);
                        let session = session.with_marked(model.marked_removal.clone());
                        let guild_id = session.guild_id;
                        let call = match effects.as_slice() {
                            [] if matches!(msg, PanelMsg::Back | PanelMsg::About) => {
                                // The terminal exits persist nothing; they
                                // only hand off to the hub beside the panel.
                                let Some(channel_id) = channel_id else {
                                    eprintln!("back/about without a channel id: panel stays");
                                    if !reply_panel(&mut out, invoke_id, &session, &model.settings)
                                    {
                                        return ExitCode::FAILURE;
                                    }
                                    continue;
                                };
                                Some(HostCall::new(
                                    Pending::OpenHub {
                                        invoke_id,
                                        session,
                                        settings: model.settings,
                                    },
                                    open_hub_args(channel_id, guild_id, hub_page),
                                ))
                            }
                            [] => {
                                // A selection-only edit re-renders in place.
                                if !reply_panel(&mut out, invoke_id, &session, &model.settings) {
                                    return ExitCode::FAILURE;
                                }
                                None
                            }
                            [Effect::Persist(snapshot)] => Some(HostCall::new(
                                Pending::Persist {
                                    invoke_id,
                                    session,
                                    settings: snapshot.clone(),
                                },
                                update_settings_args(guild_id, snapshot),
                            )),
                            _ => {
                                eprintln!("update returned an unexpected effect set");
                                None
                            }
                        };
                        if let Some(call) = call
                            && !issue_host_call(&mut out, &mut pending, &mut next_call_id, call)
                        {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::ModalSubmit {
                        invoke_id,
                        session,
                        msg,
                    } => {
                        if !ok {
                            eprintln!("{GET_SETTINGS_OP} failed: {error:?}");
                            let error = error.unwrap_or_else(|| WireError {
                                kind: "HostError".into(),
                                msg: "host call failed".into(),
                            });
                            if !reply_err(&mut out, invoke_id, &error.kind, error.msg) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        }
                        let Some(settings) = data.as_ref().and_then(parse_settings) else {
                            eprintln!("{GET_SETTINGS_OP} resp carried no snapshot");
                            if !reply_err(
                                &mut out,
                                invoke_id,
                                "HostError",
                                "host.welcome.get_settings resp carried no snapshot",
                            ) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
                        let mut model = Model {
                            settings,
                            marked_removal: session.marked_removal.clone(),
                        };
                        // A modal answer always mutates and persists: the
                        // monolith persisted even a no-op submission.
                        let effects = update(msg, &mut model);
                        let session = session.with_marked(model.marked_removal.clone());
                        let guild_id = session.guild_id;
                        let snapshot = match effects.as_slice() {
                            [Effect::Persist(snapshot)] => snapshot.clone(),
                            _ => model.settings.clone(),
                        };
                        let call = HostCall::new(
                            Pending::Persist {
                                invoke_id,
                                session,
                                settings: snapshot.clone(),
                            },
                            update_settings_args(guild_id, &snapshot),
                        );
                        if !issue_host_call(&mut out, &mut pending, &mut next_call_id, call) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::Persist {
                        invoke_id,
                        session,
                        settings,
                    } => {
                        // The persist answered the click that caused it: a
                        // failure logs but still re-renders, so the panel
                        // shows what the user chose.
                        if !ok {
                            eprintln!("{UPDATE_SETTINGS_OP} failed: {error:?}");
                        }
                        if !reply_panel(&mut out, invoke_id, &session, &settings) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::OpenHub {
                        invoke_id,
                        session,
                        settings,
                    } => {
                        if !ok {
                            eprintln!("host.open_view failed: {error:?}");
                        }
                        // The panel keeps answering its own interactions;
                        // the hub opens next to it, like every
                        // plugin→plugin navigation.
                        if !reply_panel(&mut out, invoke_id, &session, &settings) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::OpenModal { invoke_id } => {
                        // The click's answer is the modal itself: the plugin
                        // answers with the modal-opened marker so the host
                        // skips its own response and leaves the panel
                        // untouched. A failed open answers the marker too —
                        // the host's fallback edit would only re-render what
                        // never changed.
                        if !ok {
                            eprintln!("host.open_modal failed: {error:?}");
                        }
                        if !reply_err(&mut out, invoke_id, MODAL_OPENED_KIND, "modal opened") {
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
    use pwr_plugin_protocol::WelcomeSettings;
    use pwr_poise_components::IS_COMPONENTS_V2;

    use super::*;

    fn settings(welcome: WelcomeSettings) -> ServerSettings {
        ServerSettings {
            welcome,
            ..ServerSettings::default()
        }
    }

    fn model(messages: &[&str]) -> Model {
        Model {
            settings: settings(WelcomeSettings {
                enabled: Some(true),
                channel_id: Some("10".into()),
                primary_color: Some("#5865F2".into()),
                template_id: Some("2".into()),
                messages: Some(messages.iter().map(|m| m.to_string()).collect()),
            }),
            marked_removal: HashSet::new(),
        }
    }

    fn persisted(effects: &[Effect]) -> Option<ServerSettings> {
        effects
            .iter()
            .map(|e| match e {
                Effect::Persist(s) => s.clone(),
            })
            .next()
    }
    fn marked(indices: &[usize]) -> HashSet<usize> {
        indices.iter().copied().collect()
    }

    // ── update logic (ported from src/update/welcome_settings.rs) ───────────────

    #[test]
    fn toggling_enabled_persists_the_flip() {
        let mut m = model(&["one"]);
        let effects = update(PanelMsg::ToggleEnabled, &mut m);
        assert!(!m.is_enabled());
        assert_eq!(persisted(&effects).unwrap().welcome.enabled, Some(false));
    }

    #[test]
    fn a_missing_enabled_flag_reads_as_disabled() {
        let mut m = Model {
            settings: ServerSettings::default(),
            marked_removal: HashSet::new(),
        };
        assert!(!m.is_enabled());
        update(PanelMsg::ToggleEnabled, &mut m);
        assert!(m.is_enabled());
    }

    #[test]
    fn channel_and_template_selections_persist() {
        let mut m = model(&[]);
        let effects = update(PanelMsg::SetChannel(Some("99".into())), &mut m);
        assert_eq!(
            persisted(&effects).unwrap().welcome.channel_id.as_deref(),
            Some("99")
        );

        let mut m = model(&[]);
        let effects = update(PanelMsg::SetTemplate(Some("7".into())), &mut m);
        assert_eq!(
            persisted(&effects).unwrap().welcome.template_id.as_deref(),
            Some("7")
        );
    }

    #[test]
    fn a_color_without_a_leading_hash_persists_unchanged() {
        let mut m = model(&[]);
        let effects = update(PanelMsg::SetColor("ff0000".into()), &mut m);
        assert_eq!(
            persisted(&effects)
                .unwrap()
                .welcome
                .primary_color
                .as_deref(),
            Some("#5865F2"),
            "the no-op edit still persists, as the monolith did"
        );

        let mut m = model(&[]);
        update(PanelMsg::SetColor("#00ff00".into()), &mut m);
        assert_eq!(m.settings.welcome.primary_color.as_deref(), Some("#00ff00"));
    }

    #[test]
    fn adding_a_message_appends_and_persists() {
        let mut m = model(&["one"]);
        let effects = update(PanelMsg::AddMessage(" two ".into()), &mut m);
        assert_eq!(
            persisted(&effects).unwrap().welcome.messages,
            Some(vec!["one".to_string(), "two".to_string()])
        );
    }

    #[test]
    fn an_empty_message_persists_the_unchanged_list() {
        let mut m = model(&["one"]);
        let effects = update(PanelMsg::AddMessage("   ".into()), &mut m);
        assert_eq!(
            persisted(&effects).unwrap().welcome.messages.as_deref(),
            Some(&["one".to_string()][..])
        );
        assert_eq!(m.message_count(), 1);
    }

    #[test]
    fn the_message_cap_rejects_an_addition_and_still_persists() {
        let many: Vec<String> = (0..MAX_MESSAGES).map(|i| i.to_string()).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let mut m = model(&many);
        let effects = update(PanelMsg::AddMessage("one too many".into()), &mut m);
        assert_eq!(m.message_count(), MAX_MESSAGES);
        assert_eq!(
            persisted(&effects).unwrap().welcome.messages.unwrap().len(),
            MAX_MESSAGES
        );
    }

    #[test]
    fn marking_a_removal_persists_nothing() {
        let mut m = model(&["one", "two"]);
        let effects = update(PanelMsg::MarkRemoval(marked(&[1])), &mut m);
        assert!(effects.is_empty());
        assert_eq!(m.marked_removal, marked(&[1]));
        assert_eq!(m.message_count(), 2, "marking alone removes nothing");
    }

    #[test]
    fn saving_removals_drops_the_marked_messages_and_clears_the_marks() {
        let mut m = model(&["one", "two", "three"]);
        m.marked_removal = marked(&[0, 2]);
        let effects = update(PanelMsg::SaveRemoval, &mut m);
        assert_eq!(
            persisted(&effects).unwrap().welcome.messages,
            Some(vec!["two".to_string()])
        );
        assert!(m.marked_removal.is_empty());
    }

    #[test]
    fn cancelling_clears_the_marks_and_persists_nothing() {
        let mut m = model(&["one"]);
        m.marked_removal = marked(&[0]);
        let effects = update(PanelMsg::CancelRemoval, &mut m);
        assert!(effects.is_empty());
        assert!(m.marked_removal.is_empty());
    }

    #[test]
    fn the_terminal_messages_persist_nothing() {
        for msg in [PanelMsg::Back, PanelMsg::About] {
            let mut m = model(&["one"]);
            assert_eq!(update(msg.clone(), &mut m), Vec::new(), "{msg:?}");
        }
    }

    // ── removal labels ──────────────────────────────────────────────────────

    #[test]
    fn a_short_removal_label_carries_the_message_verbatim() {
        assert_eq!(removal_label("short", false), "short");
        assert_eq!(removal_label("short", true), "❌ short");
    }

    #[test]
    fn a_long_removal_label_is_truncated_at_a_char_boundary() {
        let long = "x".repeat(60);
        assert_eq!(
            removal_label(&long, false),
            format!("{}...", "x".repeat(47))
        );
        assert_eq!(
            removal_label(&long, true),
            format!("❌ {}...", "x".repeat(45))
        );

        let multibyte = "ü".repeat(60);
        assert_eq!(removal_label(&multibyte, false).chars().count(), 50);
        assert!(removal_label(&multibyte, false).ends_with("ü..."));
    }

    // ── session state ───────────────────────────────────────────────────────

    #[test]
    fn session_state_round_trips_through_value() {
        let session = SessionState::new(42).with_marked(marked(&[0, 3]));
        let parsed = SessionState::from_value(Some(&session.to_value()));
        assert_eq!(parsed, Some(session));
    }

    #[test]
    fn session_state_carries_no_settings() {
        assert_eq!(
            SessionState::new(7).to_value(),
            json!({ "guild_id": 7, "marked_removal": [] })
        );
    }

    #[test]
    fn a_missing_marked_removal_reads_as_empty() {
        assert_eq!(
            SessionState::from_value(Some(&json!({ "guild_id": 7 }))),
            Some(SessionState::new(7))
        );
    }

    #[test]
    fn malformed_session_state_yields_none() {
        assert_eq!(SessionState::from_value(None), None);
        assert_eq!(SessionState::from_value(Some(&json!({}))), None);
        assert_eq!(
            SessionState::from_value(Some(&json!({ "guild_id": 7, "marked_removal": "nope" }))),
            None
        );
    }

    // ── view rendering ──────────────────────────────────────────────────────

    #[test]
    fn panel_is_components_v2_without_legacy_content() {
        let data = view_data(&model(&["one"]));
        assert_eq!(data["flags"], json!(IS_COMPONENTS_V2));
        assert!(data.get("content").is_none(), "v2 carries no top content");
    }

    #[test]
    fn panel_mirrors_the_monolith_layout() {
        let data = view_data(&model(&["one"]));
        let components = data["components"].as_array().expect("components");
        assert_eq!(components.len(), 2, "container plus the nav row");

        let children = components[0]["components"].as_array().expect("children");
        assert_eq!(
            children[0]["content"],
            json!(
                "-# **Settings > Welcome**\n## Welcome Settings\n\n> 🛈  Welcome cards are **active**."
            )
        );
        assert_eq!(
            children[1]["components"][0]["custom_id"],
            json!(CUSTOM_ID_TOGGLE)
        );
        assert_eq!(children[1]["components"][0]["label"], json!("Disable"));
        assert_eq!(children[1]["components"][0]["style"], json!(4));
        assert_eq!(
            children[2]["components"][0]["custom_id"],
            json!(CUSTOM_ID_CHANNEL)
        );
        assert_eq!(
            children[3]["components"][0]["custom_id"],
            json!(CUSTOM_ID_TEMPLATE)
        );
        assert_eq!(
            children[3]["components"][0]["placeholder"],
            json!("Select Template (Current: 2)")
        );

        let row = children[4]["components"]
            .as_array()
            .expect("color/add/preview");
        assert_eq!(row.len(), 3);
        assert_eq!(row[0]["custom_id"], json!(CUSTOM_ID_COLOR));
        assert_eq!(row[0]["label"], json!("Set Color"));
        assert_eq!(row[1]["custom_id"], json!(CUSTOM_ID_ADD));
        assert_eq!(row[1]["label"], json!("Add Welcome Message"));
        assert_eq!(row[2]["label"], json!("Preview Templates"));
        assert_eq!(
            row[2]["url"],
            json!("https://github.com/FAZuH/pwr-bot/blob/main/docs/welcome_templates_preview.png")
        );
        assert!(
            row[2].get("custom_id").is_none(),
            "a link button has no custom_id"
        );

        assert_eq!(children[5]["content"], json!(VARIABLES_TEXT));
        assert_eq!(
            children[6]["components"][0]["custom_id"],
            json!(CUSTOM_ID_REMOVE)
        );

        let nav = components[1]["components"].as_array().expect("nav");
        assert_eq!(nav[0]["custom_id"], json!(CUSTOM_ID_BACK));
        assert_eq!(nav[0]["label"], json!("❮ Back"));
        assert_eq!(nav[1]["custom_id"], json!(CUSTOM_ID_ABOUT));
        assert_eq!(nav[1]["label"], json!("🛈 About"));
    }

    #[test]
    fn a_disabled_panel_renders_the_off_copy_and_enable_button() {
        let mut m = model(&[]);
        m.settings.welcome.enabled = Some(false);
        let data = view_data(&m);
        let children = data["components"][0]["components"].as_array().unwrap();
        assert!(
            children[0]["content"]
                .as_str()
                .unwrap()
                .contains("**disabled**")
        );
        assert_eq!(children[1]["components"][0]["label"], json!("Enable"));
        assert_eq!(children[1]["components"][0]["style"], json!(3));
    }

    #[test]
    fn the_removal_select_only_appears_once_a_message_exists() {
        let with = view_data(&model(&["one"]));
        let without = view_data(&model(&[]));
        let children = with["components"][0]["components"].as_array().unwrap();
        assert_eq!(children.len(), 7, "six fixed rows plus the removal select");
        let children = without["components"][0]["components"].as_array().unwrap();
        assert_eq!(children.len(), 6);
    }

    #[test]
    fn the_save_row_only_appears_once_something_is_marked() {
        let mut m = model(&["one", "two"]);
        let without = view_data(&m);
        let children = without["components"][0]["components"].as_array().unwrap();
        assert_eq!(children.len(), 7);

        m.marked_removal = marked(&[1]);
        let with = view_data(&m);
        let children = with["components"][0]["components"].as_array().unwrap();
        assert_eq!(children.len(), 8);
        let row = children[7]["components"].as_array().unwrap();
        assert_eq!(row[0]["custom_id"], json!(CUSTOM_ID_SAVE));
        assert_eq!(row[0]["label"], json!("Save Removals"));
        assert_eq!(row[1]["custom_id"], json!(CUSTOM_ID_CANCEL));
        assert_eq!(row[1]["label"], json!("Cancel"));
    }

    #[test]
    fn a_marked_option_is_flagged_and_defaulted() {
        let mut m = model(&["short", &"y".repeat(60)]);
        m.marked_removal = marked(&[1]);
        let data = view_data(&m);
        let menu = &data["components"][0]["components"][6]["components"][0];
        let options = menu["options"].as_array().unwrap();
        assert_eq!(options[0]["label"], json!("short"));
        assert!(options[0].get("default").is_none());
        assert_eq!(options[1]["label"], format!("❌ {}...", "y".repeat(45)));
        assert_eq!(options[1]["default"], json!(true));
        assert_eq!(menu["max_values"], json!(2));
    }

    #[test]
    fn the_add_button_disappears_at_the_cap() {
        let many: Vec<String> = (0..MAX_MESSAGES).map(|i| i.to_string()).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let data = view_data(&model(&many));
        let row = data["components"][0]["components"][4]["components"]
            .as_array()
            .unwrap();
        assert_eq!(row.len(), 2, "Set Color plus the preview link");
        assert_eq!(row[1]["label"], json!("Preview Templates"));
    }

    #[test]
    fn the_channel_select_defaults_to_the_stored_channel() {
        let data = view_data(&model(&[]));
        let menu = &data["components"][0]["components"][2]["components"][0];
        assert_eq!(menu["type"], json!(8));
        assert_eq!(menu["channel_types"], json!([0]));
        assert_eq!(menu["default_values"][0]["id"], json!(10u64));
        assert_eq!(menu["default_values"][0]["type"], json!("channel"));
    }

    // ── attachment declaration (ADR-0012) ───────────────────────────────────

    #[test]
    fn the_preview_slot_is_declared_while_enabled() {
        assert_eq!(
            attachment_declaration(&model(&["one"])),
            json!([{ "id": 0, "filename": WELCOME_FILE }])
        );
    }

    #[test]
    fn a_disabled_panel_declares_an_empty_slot_list() {
        let mut m = model(&["one"]);
        m.settings.welcome.enabled = Some(false);
        assert_eq!(
            attachment_declaration(&m),
            json!([]),
            "an explicit empty list removes the attachment on edit"
        );
    }

    #[test]
    fn the_envelope_carries_the_declaration_and_session_state() {
        let m = model(&["one"]);
        let session = SessionState::new(7);
        let data = envelope(&session, &m);
        assert_eq!(
            data["data"]["attachments"],
            json!([{ "id": 0, "filename": WELCOME_FILE }])
        );
        assert_eq!(data["ephemeral"], json!(false));
        assert_eq!(data["view"], session.to_value());
        assert_eq!(data["data"]["flags"], json!(IS_COMPONENTS_V2));
    }

    // ── modals ──────────────────────────────────────────────────────────────

    #[test]
    fn a_modal_custom_id_is_nonce_suffixed() {
        assert_eq!(Modal::AddMessage.custom_id(3), "welcome:add:3");
        assert_eq!(Modal::SetColor.custom_id(4), "welcome:color:4");
    }

    #[test]
    fn a_modal_spec_is_a_label_wrapped_text_input() {
        let spec = Modal::AddMessage.spec("welcome:add:1");
        assert_eq!(spec["custom_id"], json!("welcome:add:1"));
        assert_eq!(spec["title"], json!("Add Welcome Message"));
        let label = &spec["components"][0];
        assert_eq!(label["type"], json!(18));
        assert_eq!(label["label"], json!("Message"));
        let input = &label["component"];
        assert_eq!(input["type"], json!(4));
        assert_eq!(input["style"], json!(2), "paragraph");
        assert_eq!(input["custom_id"], json!(INPUT_MESSAGE));
        assert_eq!(input["max_length"], json!(200));

        let spec = Modal::SetColor.spec("welcome:color:2");
        assert_eq!(spec["title"], json!("Set Primary Color"));
        let input = &spec["components"][0]["component"];
        assert_eq!(input["style"], json!(1), "short");
        assert_eq!(input["custom_id"], json!(INPUT_COLOR));
        assert_eq!(input["placeholder"], json!("#5865F2"));
    }

    #[test]
    fn a_trigger_id_names_its_modal() {
        assert_eq!(Modal::from_trigger(CUSTOM_ID_ADD), Some(Modal::AddMessage));
        assert_eq!(Modal::from_trigger(CUSTOM_ID_COLOR), Some(Modal::SetColor));
        assert_eq!(Modal::from_trigger(CUSTOM_ID_TOGGLE), None);
    }

    #[test]
    fn a_submission_id_names_its_modal_by_prefix() {
        assert_eq!(
            Modal::from_submission("welcome:add:7"),
            Some(Modal::AddMessage)
        );
        assert_eq!(
            Modal::from_submission("welcome:color:12"),
            Some(Modal::SetColor)
        );
        assert_eq!(Modal::from_submission("welcome:add"), None);
        assert_eq!(Modal::from_submission("voice:whatever"), None);
    }

    #[test]
    fn the_modal_open_args_carry_the_interaction_identity() {
        let args = open_modal_args(5, 6, "tok", Modal::SetColor.spec("welcome:color:1"));
        assert_eq!(args["author_id"], json!(5));
        assert_eq!(args["interaction_id"], json!(6));
        assert_eq!(args["token"], json!("tok"));
        assert_eq!(args["modal"]["custom_id"], json!("welcome:color:1"));
    }

    #[test]
    fn the_interaction_identity_reads_the_merged_raw_interaction() {
        let args = json!({ "id": "6", "token": "tok", "user": { "id": "5" } });
        assert_eq!(
            interaction_identity(Some(&args)),
            Some((6, "tok".to_string(), 5))
        );
        assert_eq!(interaction_identity(Some(&json!({ "id": 6 }))), None);
        assert_eq!(
            interaction_identity(Some(&json!({ "id": 6, "token": "t" }))),
            None
        );
    }

    #[test]
    fn a_modal_input_value_is_found_by_custom_id() {
        let args = json!({
            "data": { "components": [
                { "type": 18, "component": { "custom_id": INPUT_COLOR, "value": "#00ff00" } }
            ]}
        });
        assert_eq!(
            modal_input_value(Some(&args), INPUT_COLOR).as_deref(),
            Some("#00ff00")
        );
        assert_eq!(modal_input_value(Some(&args), INPUT_MESSAGE), None);
        assert_eq!(modal_input_value(None, INPUT_COLOR), None);
    }

    // ── click routing ───────────────────────────────────────────────────────

    #[test]
    fn a_click_id_maps_to_its_message() {
        let values = json!({ "data": { "values": ["7"] } });
        assert_eq!(
            click_msg(CUSTOM_ID_TOGGLE, None),
            Some(PanelMsg::ToggleEnabled)
        );
        assert_eq!(
            click_msg(CUSTOM_ID_CHANNEL, Some(&values)),
            Some(PanelMsg::SetChannel(Some("7".into())))
        );
        assert_eq!(
            click_msg(CUSTOM_ID_TEMPLATE, Some(&values)),
            Some(PanelMsg::SetTemplate(Some("7".into())))
        );
        assert_eq!(
            click_msg(
                CUSTOM_ID_REMOVE,
                Some(&json!({ "data": { "values": ["0", "x", "2"] } }))
            ),
            Some(PanelMsg::MarkRemoval(marked(&[0, 2])))
        );
        assert_eq!(click_msg(CUSTOM_ID_SAVE, None), Some(PanelMsg::SaveRemoval));
        assert_eq!(
            click_msg(CUSTOM_ID_CANCEL, None),
            Some(PanelMsg::CancelRemoval)
        );
        assert_eq!(click_msg(CUSTOM_ID_BACK, None), Some(PanelMsg::Back));
        assert_eq!(click_msg(CUSTOM_ID_ABOUT, None), Some(PanelMsg::About));
    }

    #[test]
    fn the_modal_triggers_are_not_click_messages() {
        assert_eq!(click_msg(CUSTOM_ID_ADD, None), None);
        assert_eq!(click_msg(CUSTOM_ID_COLOR, None), None);
        assert_eq!(click_msg("welcome:nonsense", None), None);
    }

    #[test]
    fn an_empty_select_clears_the_channel() {
        assert_eq!(
            click_msg(
                CUSTOM_ID_CHANNEL,
                Some(&json!({ "data": { "values": [] } }))
            ),
            Some(PanelMsg::SetChannel(None))
        );
    }

    // ── protocol ────────────────────────────────────────────────────────────

    #[test]
    fn the_manifest_declares_no_command_and_no_handler() {
        let m = manifest();
        assert_eq!(m.name, PLUGIN_NAME);
        assert!(m.commands.is_empty(), "the hub opens this panel");
        assert!(m.event_handlers.is_empty(), "expiry persists nothing");
        assert_eq!(m.api_version, API_VERSION);
    }

    #[test]
    fn get_and_update_args_carry_the_guild_and_snapshot() {
        assert_eq!(get_settings_args(7), json!({ "guild_id": 7 }));
        let settings = model(&["one"]).settings;
        let args = update_settings_args(7, &settings);
        assert_eq!(args["guild_id"], json!(7));
        assert_eq!(
            serde_json::from_value::<ServerSettings>(args["settings"].clone()).unwrap(),
            settings
        );
    }

    #[test]
    fn parse_settings_accepts_a_snapshot_and_rejects_garbage() {
        let settings = model(&[]).settings;
        assert_eq!(
            parse_settings(&serde_json::to_value(&settings).unwrap()),
            Some(settings)
        );
        assert_eq!(parse_settings(&json!("nope")), None);
    }

    #[test]
    fn pending_ops_name_their_host_ops() {
        let session = SessionState::new(1);
        assert_eq!(
            Pending::LoadSettings {
                invoke_id: 0,
                guild_id: 1
            }
            .op(),
            GET_SETTINGS_OP
        );
        assert_eq!(
            Pending::Interact {
                invoke_id: 0,
                session: session.clone(),
                msg: PanelMsg::ToggleEnabled,
                channel_id: None,
                hub_page: HubPage::Hub,
            }
            .op(),
            GET_SETTINGS_OP
        );
        assert_eq!(
            Pending::Persist {
                invoke_id: 0,
                session: session.clone(),
                settings: ServerSettings::default(),
            }
            .op(),
            UPDATE_SETTINGS_OP
        );
        assert_eq!(
            Pending::OpenHub {
                invoke_id: 0,
                session: session.clone(),
                settings: ServerSettings::default(),
            }
            .op(),
            "host.open_view"
        );
        assert_eq!(Pending::OpenModal { invoke_id: 0 }.op(), "host.open_modal");
        assert_eq!(
            Pending::ModalSubmit {
                invoke_id: 0,
                session,
                msg: PanelMsg::AddMessage("x".into()),
            }
            .op(),
            GET_SETTINGS_OP
        );
    }

    #[test]
    fn an_issued_call_writes_one_line_and_records_its_pending_kind() {
        let mut out: Vec<u8> = Vec::new();
        let mut pending = HashMap::new();
        let mut next_call_id = 0;
        let ok = issue_host_call(
            &mut out,
            &mut pending,
            &mut next_call_id,
            HostCall::new(
                Pending::LoadSettings {
                    invoke_id: 9,
                    guild_id: 1,
                },
                get_settings_args(1),
            ),
        );
        assert!(ok);
        assert_eq!(next_call_id, 1);
        assert_eq!(pending.len(), 1);
        let written = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 1);
        let msg: Msg = serde_json::from_str(lines[0]).unwrap();
        match msg {
            Msg::Call { id, op, args, .. } => {
                assert_eq!(id, 1);
                assert_eq!(op, GET_SETTINGS_OP);
                assert_eq!(args, Some(json!({ "guild_id": 1 })));
            }
            other => panic!("expected a call, got {other:?}"),
        }
    }
}
