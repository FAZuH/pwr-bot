//! The settings core plugin: the first real core plugin and the end-to-end
//! proof of the plugin infrastructure.
//!
//! Speaks the pwr-bot plugin wire protocol over JSON-Lines stdio, like the
//! `hello` plugin: one compact JSON object per line on stdout,
//! terminated by a single `\n` and flushed after every write; stderr is the
//! free logging channel.
//!
//! Behavior:
//! - announces `hello` (`v`, `name`, `caps`) as its first line after spawn;
//! - answers `invoke` of the `settings` command with the settings hub view,
//!   loading the persisted model from `host.kv.get` (`namespace='settings'`)
//!   on first open and applying the default model when the key is unset;
//! - renders Components V2 (`IS_COMPONENTS_V2`, no legacy content): a
//!   container holding the `-# **Settings**` header, the two info sections
//!   from the original monolith hub, a row of per-feature buttons, a string
//!   select whose ✅/⬜ labels mirror the model, plus the discovered-plugins
//!   nav row; the 🛈 About button sits outside the container;
//! - answers `view.interact` on the toggle select (`settings:toggle`) by
//!   toggling every selected feature via the settings update logic and
//!   persisting the model through `host.kv.set` before replying;
//! - `settings:about` / `settings:about:back` switch between the hub and the
//!   plugin-side About panel without touching the model;
//! - a `settings:config:<feature>` click is a navigation stub until
//!   per-feature panels exist as plugins: it re-renders the current page;
//! - a `settings:open:<plugin>` nav click issues `host.open_view` for the
//!   target plugin (the settings hub's promise: navigate to any panel),
//!   answering the interaction with the current envelope again;
//! - the nav row is built at runtime from the host's running plugins
//!   (`host.list_plugins`), minus the settings plugin itself; a host
//!   without that cap — or a manager-less spawn — falls back to the
//!   single default target;
//! - every view reply is the full envelope `{"data", "ephemeral", "view"}`
//!   the interaction engine renders verbatim;
//! - treats `event` (e.g. `view.timeout`) as one-way, never answering it;
//! - answers `ping` with `pong`, tolerates the host's hello ack silently,
//!   and exits 0 on `bye` and on EOF.

use std::collections::HashMap;
use std::io::BufRead;
use std::io::Write;
use std::process::ExitCode;

use pwr_ext::view;
use pwr_ext::view_support::ButtonStyle;
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

/// Custom ids for the hub's interactive components.
const CUSTOM_ID_CONFIG_PREFIX: &str = "settings:config:";
const CUSTOM_ID_TOGGLE: &str = "settings:toggle";
const CUSTOM_ID_ABOUT: &str = "settings:about";
const CUSTOM_ID_ABOUT_BACK: &str = "settings:about:back";

/// The configurable features, in the original hub's order: the label is both
/// the button text and the select option value (as in the monolith UI), and
/// the message is the toggle it applies.
const FEATURES: [(&str, SettingsMsg); 3] = [
    ("Feeds", SettingsMsg::Feeds),
    ("Voice", SettingsMsg::Voice),
    ("Welcome", SettingsMsg::Welcome),
];

/// Custom id prefix for the nav button: the target plugin name follows the
/// separator, so the hub can open any plugin's panel.
const CUSTOM_ID_OPEN_PREFIX: &str = "settings:open:";

/// The nav button's target while discovery has not run or the host did not
/// answer `host.list_plugins`: the hello-style fixture the integration tests
/// spawn.
const NAV_TARGET_DEFAULT: &str = "hello";

/// The nav row's targets: discovered from the host's running plugins at the
/// first view load. A `Fallback` renders the single default target; a
/// `Discovered` list renders one "Open <name>" button per entry — an empty
/// list renders no nav row at all (the #128 gate).
enum NavTargets {
    /// Discovery failed (no `host.list_plugins` cap, a manager-less spawn, or
    /// a malformed resp): fall back to [`NAV_TARGET_DEFAULT`].
    Fallback,
    /// The names of the running plugins the host reported.
    Discovered(Vec<String>),
}

/// A plugin→host call in flight: the invoke id the reply must answer, and
/// what to do with the host's resp once it arrives. The pending kinds whose
/// reply echoes an interaction's session state carry that state, parsed from
/// the interaction args at dispatch time.
#[derive(Debug, Clone, Copy)]
enum Pending {
    /// The `host.kv.get` issued to load the model before the first render.
    Load(u64),
    /// The `host.kv.set` issued to persist a toggled session model.
    Save(u64, ViewState),
    /// The `host.open_view` issued to open a target plugin's panel.
    OpenView(u64, ViewState),
    /// The `host.list_plugins` issued to discover the nav row's targets.
    ListPlugins(u64),
}

impl Pending {
    /// The host op this pending kind belongs to. The op string lives here so
    /// it stays paired with the kind that resolves its resp.
    fn op(self) -> &'static str {
        match self {
            Pending::Load(_) => "host.kv.get",
            Pending::Save(..) => "host.kv.set",
            Pending::OpenView(..) => "host.open_view",
            Pending::ListPlugins(_) => "host.list_plugins",
        }
    }
}

/// A plugin→host call decided by an incoming message: the pending kind its
/// resp will resolve, and the call's args. The op string is always
/// [`Pending::op`], never stored separately.
struct HostCall {
    pending: Pending,
    args: Value,
}

impl HostCall {
    fn new(pending: Pending, args: Value) -> Self {
        Self { pending, args }
    }
}

// ── settings update logic (moved from the host's `src/update/settings_main.rs`) ──

/// Messages that mutate the settings model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsMsg {
    Feeds,
    Voice,
    Welcome,
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
    fn to_value(self) -> Value {
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
        Feeds => model.feeds_enabled = !model.feeds_enabled,
        Voice => model.voice_enabled = !model.voice_enabled,
        Welcome => model.welcome_enabled = !model.welcome_enabled,
    }
}

// ── view rendering ───────────────────────────────────────────────────────────

/// The page a session is showing: the hub, or the plugin-side About panel
/// the original monolith reached through its own navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Hub,
    About,
}

/// The page's name in the serialized [`ViewState`].
fn page_name(page: Page) -> &'static str {
    match page {
        Page::Hub => "hub",
        Page::About => "about",
    }
}

/// Parses a [`ViewState`] page name; anything else falls back to
/// [`Page::Hub`], the page every fresh session starts on.
fn page_from_name(name: Option<&str>) -> Page {
    match name {
        Some("about") => Page::About,
        _ => Page::Hub,
    }
}

/// One view session's state: the settings model plus the page that session
/// is showing. Serialized as the envelope's opaque `view` payload, which the
/// host stores per message and echoes back on every interaction — so two
/// concurrently open hubs keep independent pages and models instead of
/// sharing process-global state. The tradeoff is lost updates: each toggle
/// persists its session's full model, so two hubs open at once last-writer-
/// wins against each other — acceptable for three boolean features, and a
/// reopen of the hub picks up whatever was persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ViewState {
    model: SettingsModel,
    page: Page,
}

impl Default for ViewState {
    /// A fresh session: default model, hub page.
    fn default() -> Self {
        Self {
            model: SettingsModel::default(),
            page: Page::Hub,
        }
    }
}

impl ViewState {
    /// The state as the envelope's `view` value. The KV copy of the model
    /// stays bare ([`SettingsModel::to_value`]); only the wire payload nests
    /// it under `model`.
    fn to_value(self) -> Value {
        json!({
            "model": self.model.to_value(),
            "page": page_name(self.page),
        })
    }

    /// Parses a host-echoed `view` value; a missing or malformed payload
    /// yields a fresh session.
    fn from_value(value: Option<&Value>) -> Self {
        let value = match value {
            Some(value) => value,
            None => return Self::default(),
        };
        Self {
            model: value
                .get("model")
                .map(SettingsModel::from_value)
                .unwrap_or_default(),
            page: page_from_name(value.get("page").and_then(Value::as_str)),
        }
    }
}

/// The enabled state of one feature: the select labels mirror it.
fn feature_enabled(model: &SettingsModel, msg: SettingsMsg) -> bool {
    match msg {
        SettingsMsg::Feeds => model.feeds_enabled,
        SettingsMsg::Voice => model.voice_enabled,
        SettingsMsg::Welcome => model.welcome_enabled,
    }
}

/// Maps a select option value (the feature label, as in the original UI) to
/// its toggle message; unknown labels are ignored by the caller.
fn toggle_msg_for(label: &str) -> Option<SettingsMsg> {
    FEATURES
        .iter()
        .find(|(name, _)| *name == label)
        .map(|(_, msg)| *msg)
}

/// Info text under the Configure heading, verbatim from the original hub.
const CONFIGURE_INFO: &str = concat!(
    "### Configure Feature Settings\n",
    "> 🛈  Click a button to edit settings for a specific feature.",
);

/// Info text under the Enable/Disable heading, verbatim from the original.
const TOGGLE_INFO: &str = concat!(
    "### Enable or Disable Features\n",
    "> 🛈  Turn features on or off. A checkmark means the feature is currently enabled.",
);

/// About panel copy: the monolith's Info section without the live stats
/// (uptime, servers, ...) — no host op exposes them to plugins yet.
fn about_copy() -> String {
    format!(
        concat!(
            "-# **Settings > About**\n",
            "## pwr-bot\n",
            "### Info\n",
            "- **Author**: [FAZuH](https://github.com/FAZuH)\n",
            "- **Source**: [GitHub](https://github.com/FAZuH/pwr-bot)\n",
            "- **License**: [MIT](https://github.com/FAZuH/pwr-bot/blob/main/LICENSE)\n",
            "Copyright © FAZuH — v{}"
        ),
        env!("CARGO_PKG_VERSION")
    )
}

/// Renders the settings hub as Components V2, mirroring the original monolith
/// layout: one container with the header, both info sections, the per-feature
/// button row, and the toggle select whose ✅/⬜ labels mirror the model;
/// the discovered-plugins nav row follows inside the container, and the 🛈
/// About button sits outside it.
///
/// The composition stays on the `pwr_poise_components` builders rather than a
/// single `view!` literal: the nav row is runtime data (zero to N buttons
/// built from `host.list_plugins`, dropped entirely when the list is empty)
/// inside an otherwise fixed container, and `view!` children are compile-time
/// literals — the grammar cannot splice a runtime element list, cannot
/// conditionally include one, and cannot author a standalone component to
/// embed in a runtime parent. Every child is therefore a runtime `Value`,
/// which is the reusable-library role the components crate keeps. The
/// per-feature buttons and toggle options stay derived from [`FEATURES`] so
/// the feature list dispatch reads is not forked into a literal view.
/// Pagination has no fit here either: the nav row is the only list-like
/// piece, and its handful of `Open <plugin>` buttons already fit one action
/// row (Discord caps a row at five buttons) — smaller than a pagination
/// control's five-button indicator.
fn view_data(model: &SettingsModel, nav: &NavTargets) -> Value {
    let header = components::text_display("-# **Settings**");
    let configure_info = components::text_display(CONFIGURE_INFO);
    let config_buttons = FEATURES.iter().map(|(label, _)| {
        components::button_with_style(
            format!("{CUSTOM_ID_CONFIG_PREFIX}{}", label.to_lowercase()),
            *label,
            2,
        )
    });
    let toggle_info = components::text_display(TOGGLE_INFO);
    let options = FEATURES.iter().map(|(label, msg)| {
        let emoji = if feature_enabled(model, *msg) {
            "✅"
        } else {
            "⬜"
        };
        components::select_option(format!("{emoji} {label}"), *label)
    });
    let open = match nav {
        NavTargets::Fallback => nav_buttons(&[NAV_TARGET_DEFAULT]),
        NavTargets::Discovered(targets) => {
            let refs: Vec<&str> = targets.iter().map(String::as_str).collect();
            nav_buttons(&refs)
        }
    };

    let mut children = vec![
        header,
        configure_info,
        components::action_row(config_buttons),
        toggle_info,
        components::action_row([components::string_select(CUSTOM_ID_TOGGLE, options)]),
    ];
    if !open.is_empty() {
        children.push(components::action_row(open));
    }
    components::view_data_v2([
        components::container(children),
        components::action_row([components::button_with_style(CUSTOM_ID_ABOUT, "🛈 About", 2)]),
    ])
}

/// Renders the plugin-side About panel: a container holding a section with
/// the About copy (a Source Code link button as its accessory) plus the
/// License link row, and the ❮ Back button outside. The live stats of the
/// monolith panel need host capabilities no host op exposes yet.
fn about_view() -> Value {
    let copy = about_copy();
    let message = view! {
        components_v2 {
            container {
                section {
                    text_display { content: copy }
                    button {
                        url: "https://github.com/FAZuH/pwr-bot",
                        label: "Source Code"
                    }
                }
                action_row {
                    button {
                        url: "https://github.com/FAZuH/pwr-bot/blob/main/LICENSE",
                        label: "License"
                    }
                }
            }
            action_row {
                button {
                    custom_id: CUSTOM_ID_ABOUT_BACK,
                    label: "❮ Back",
                    style: ButtonStyle::Secondary
                }
            }
        }
    };
    serde_json::to_value(message).expect("settings about view is serializable")
}

/// The nav row's buttons, one per declared target: empty when no target is
/// declared, so a plugin the hub cannot open never renders a dead button.
fn nav_buttons(targets: &[&str]) -> Vec<Value> {
    targets.iter().map(|target| nav_button(target)).collect()
}

/// The nav button opening another plugin's panel: the target name rides in
/// the custom id (`settings:open:<target>`), and the label names the target.
fn nav_button(target: &str) -> Value {
    components::button(
        format!("{CUSTOM_ID_OPEN_PREFIX}{target}"),
        format!("Open {target}"),
    )
}

/// The full envelope a view reply carries: raw message data, visibility, and
/// the session state the host stores per message and hands back on
/// interactions.
fn envelope(state: &ViewState, nav: &NavTargets) -> Value {
    let data = match state.page {
        Page::Hub => view_data(&state.model, nav),
        Page::About => about_view(),
    };
    json!({
        "data": data,
        "ephemeral": false,
        "view": state.to_value(),
    })
}

/// Writes an ok resp answering `invoke_id` with the given session's envelope.
/// Returns whether the write succeeded.
fn reply_envelope(
    out: &mut impl Write,
    invoke_id: u64,
    state: &ViewState,
    nav: &NavTargets,
) -> bool {
    let resp = Msg::resp_ok(invoke_id, Some(envelope(state, nav)));
    write_msg(out, &resp).is_ok()
}

/// The page a click needing no host call lands on: About opens the panel,
/// Back returns to the hub, and a Configure stub keeps the current page.
/// `None` when the custom id is none of those.
fn page_swap(custom_id: Option<&str>, current: Page) -> Option<Page> {
    match custom_id {
        Some(CUSTOM_ID_ABOUT) => Some(Page::About),
        Some(CUSTOM_ID_ABOUT_BACK) => Some(Page::Hub),
        Some(clicked)
            if clicked
                .strip_prefix(CUSTOM_ID_CONFIG_PREFIX)
                .is_some_and(|target| !target.is_empty()) =>
        {
            Some(current)
        }
        _ => None,
    }
}

/// Parses a `host.list_plugins` resp into the running plugin names, minus
/// the plugin itself: the hub never renders a self-opening nav row. `None`
/// on a missing, non-object, or malformed payload, so the caller falls back
/// to [`NavTargets::Fallback`].
fn parse_list_plugins(data: Option<&Value>) -> Option<Vec<String>> {
    let plugins = data?.get("plugins")?.as_array()?;
    let names: Vec<&str> = plugins.iter().map(Value::as_str).collect::<Option<_>>()?;
    Some(
        names
            .into_iter()
            .filter(|name| *name != PLUGIN_NAME)
            .map(str::to_string)
            .collect(),
    )
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

/// The `host.open_view` call args opening the target plugin's panel: the
/// channel the source interaction came from, the target name as both the
/// plugin and the command, and no invoke args.
fn open_view_args(channel_id: u64, plugin: &str) -> Value {
    json!({
        "channel_id": channel_id,
        "plugin": plugin,
        "command": plugin,
        "args": {},
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

/// Writes a `resp_err` answering `invoke_id` with the given error kind and
/// message; returns whether the write succeeded.
fn reply_err(out: &mut impl Write, invoke_id: u64, kind: &str, msg: impl Into<String>) -> bool {
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
    pending.insert(*next_call_id, pending_kind);
    let call_msg = Msg::Call {
        id: *next_call_id,
        op: pending_kind.op().into(),
        cmd: None,
        args: Some(args),
    };
    write_msg(out, &call_msg).is_ok()
}

/// Answers an invoke after its chained host call completed: a failed host
/// call is logged, not fatal — the envelope still renders with the session's
/// state. Returns whether the write succeeded.
fn answer_envelope(
    out: &mut impl Write,
    invoke_id: u64,
    ok: bool,
    error: Option<WireError>,
    pending_kind: Pending,
    state: &ViewState,
    nav: &NavTargets,
) -> bool {
    if !ok {
        eprintln!(
            "{} failed: {:?}",
            pending_kind.op(),
            error.unwrap_or_else(|| WireError {
                kind: "HostError".into(),
                msg: "host call failed".into(),
            })
        );
    }
    reply_envelope(out, invoke_id, state, nav)
}

fn main() -> ExitCode {
    if let Err(e) = manifest().validate() {
        eprintln!("manifest invalid: {e}");
        return ExitCode::FAILURE;
    }

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    // The latest model known to be persisted: loaded from KV on the first
    // invoke, updated by toggles. It seeds every NEW session (each `/settings`
    // opens one); live sessions carry their own state in their envelope.
    let mut model: Option<SettingsModel> = None;
    let mut nav = NavTargets::Fallback;
    let mut next_call_id: u64 = 0;
    // plugin->host calls in flight: our call id -> the pending kind whose resp
    // completes this call chain.
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
            "host.list_plugins".into(),
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
                // with a fresh hub session; the first invoke loads from KV
                // first. Every invoke opens the hub — pages belong to the
                // sessions their messages carry.
                if (op.as_str(), cmd.as_deref()) == ("invoke", Some(PLUGIN_NAME)) && model.is_some()
                {
                    let state = ViewState {
                        model: model.unwrap_or_default(),
                        page: Page::Hub,
                    };
                    if !reply_envelope(&mut out, id, &state, &nav) {
                        return ExitCode::FAILURE;
                    }
                    continue;
                }
                let host_call = match (op.as_str(), cmd.as_deref()) {
                    ("invoke", Some(PLUGIN_NAME)) => {
                        Some(HostCall::new(Pending::Load(id), kv_get_args()))
                    }
                    ("view.interact", Some(PLUGIN_NAME)) => {
                        let custom_id = args
                            .as_ref()
                            .and_then(|a| a.get("custom_id"))
                            .and_then(Value::as_str);
                        // The session state the host echoed back: this
                        // message's own model and page, not process globals.
                        let session =
                            ViewState::from_value(args.as_ref().and_then(|a| a.get("view")));
                        // A nav click opens another plugin's panel: forward
                        // the source interaction's channel and the target
                        // parsed from the custom id to host.open_view.
                        if let Some(custom_id) = custom_id
                            && let Some(target) = custom_id.strip_prefix(CUSTOM_ID_OPEN_PREFIX)
                            && !target.is_empty()
                        {
                            let Some(channel_id) = args
                                .as_ref()
                                .and_then(|a| a.get("channel_id"))
                                .and_then(Value::as_u64)
                            else {
                                if !reply_err(
                                    &mut out,
                                    id,
                                    "InvalidArgs",
                                    "missing `channel_id` (u64)",
                                ) {
                                    return ExitCode::FAILURE;
                                }
                                continue;
                            };
                            Some(HostCall::new(
                                Pending::OpenView(id, session),
                                open_view_args(channel_id, target),
                            ))
                        } else if let Some(next_page) = page_swap(custom_id, session.page) {
                            // Pages needing no host call: About/Back swap the
                            // session's page; a Configure button is a
                            // navigation stub until per-feature panels exist
                            // as plugins — it re-renders the current page
                            // rather than failing the interaction.
                            let state = ViewState {
                                model: session.model,
                                page: next_page,
                            };
                            if !reply_envelope(&mut out, id, &state, &nav) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        } else if custom_id == Some(CUSTOM_ID_TOGGLE) {
                            // The select values ride the merged interaction
                            // under `data.values`; every selected feature is
                            // toggled on the session's model, unknown labels
                            // ignored (the original select did the same).
                            let values = args
                                .as_ref()
                                .and_then(|a| a.get("data"))
                                .and_then(|d| d.get("values"))
                                .and_then(Value::as_array);
                            let Some(values) = values else {
                                if !reply_err(
                                    &mut out,
                                    id,
                                    "InvalidArgs",
                                    "missing `data.values` (array of feature names)",
                                ) {
                                    return ExitCode::FAILURE;
                                }
                                continue;
                            };
                            let mut current = session.model;
                            for value in values.iter().filter_map(Value::as_str) {
                                if let Some(msg) = toggle_msg_for(value) {
                                    update(msg, &mut current);
                                }
                            }
                            // The cache seeds future sessions with what this
                            // toggle just persisted.
                            model = Some(current);
                            let state = ViewState {
                                model: current,
                                page: session.page,
                            };
                            Some(HostCall::new(
                                Pending::Save(id, state),
                                kv_set_args(&current),
                            ))
                        } else {
                            if !reply_err(
                                &mut out,
                                id,
                                "UnknownAction",
                                format!("unknown custom_id: {custom_id:?}"),
                            ) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        }
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
                        // failed; then discover the nav row's targets before
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
                        let call = HostCall::new(Pending::ListPlugins(invoke_id), json!({}));
                        if !issue_host_call(&mut out, &mut pending, &mut next_call_id, call) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::ListPlugins(invoke_id) => {
                        // The running plugin names, or the default target when
                        // discovery failed; then persist the panel state
                        // before the first render. A first render is always a
                        // fresh hub session.
                        match parse_list_plugins(data.as_ref()) {
                            Some(targets) => nav = NavTargets::Discovered(targets),
                            None => {
                                eprintln!("host.list_plugins failed: {error:?}");
                                nav = NavTargets::Fallback;
                            }
                        }
                        let state = ViewState {
                            model: model.unwrap_or_default(),
                            page: Page::Hub,
                        };
                        let call = HostCall::new(
                            Pending::Save(invoke_id, state),
                            kv_set_args(&state.model),
                        );
                        if !issue_host_call(&mut out, &mut pending, &mut next_call_id, call) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::Save(invoke_id, state) | Pending::OpenView(invoke_id, state) => {
                        if !answer_envelope(
                            &mut out,
                            invoke_id,
                            ok,
                            error,
                            pending_kind,
                            &state,
                            &nav,
                        ) {
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
        update(SettingsMsg::Feeds, &mut model);
        assert!(model.feeds_enabled);
        assert!(!model.voice_enabled);
        update(SettingsMsg::Feeds, &mut model);
        assert!(!model.feeds_enabled);
    }

    #[test]
    fn toggle_voice_flips_voice() {
        let mut model = SettingsModel::default();
        update(SettingsMsg::Voice, &mut model);
        assert!(model.voice_enabled);
    }

    #[test]
    fn toggle_welcome_flips_welcome() {
        let mut model = SettingsModel::default();
        update(SettingsMsg::Welcome, &mut model);
        assert!(model.welcome_enabled);
    }

    #[test]
    fn multiple_toggles_are_independent() {
        let mut model = SettingsModel::default();
        update(SettingsMsg::Feeds, &mut model);
        update(SettingsMsg::Welcome, &mut model);
        assert!(model.feeds_enabled);
        assert!(!model.voice_enabled);
        assert!(model.welcome_enabled);
    }

    #[test]
    fn model_round_trips_through_value() {
        let mut model = SettingsModel::default();
        update(SettingsMsg::Feeds, &mut model);
        update(SettingsMsg::Voice, &mut model);
        let parsed = SettingsModel::from_value(&model.to_value());
        assert_eq!(parsed, model);
    }

    #[test]
    fn malformed_value_falls_back_to_default() {
        let parsed = SettingsModel::from_value(&json!({"feeds": "nope"}));
        assert_eq!(parsed, SettingsModel::default());
    }

    #[test]
    fn envelope_carries_data_ephemeral_and_the_session_view() {
        let state = ViewState::default();
        let envelope = envelope(&state, &NavTargets::Fallback);
        assert!(envelope.get("data").is_some());
        assert_eq!(envelope["ephemeral"], false);
        assert_eq!(envelope["view"]["page"], json!("hub"));
        assert_eq!(envelope["view"]["model"]["feeds"], json!(false));
    }

    #[test]
    fn view_state_round_trips_through_value() {
        let state = ViewState {
            model: SettingsModel {
                feeds_enabled: true,
                voice_enabled: false,
                welcome_enabled: true,
            },
            page: Page::About,
        };
        let parsed = ViewState::from_value(Some(&state.to_value()));
        assert_eq!(parsed, state);
    }

    #[test]
    fn a_malformed_or_missing_view_state_yields_a_fresh_session() {
        assert_eq!(ViewState::from_value(None), ViewState::default());
        assert_eq!(
            ViewState::from_value(Some(&json!({}))),
            ViewState::default(),
            "an empty object is a fresh hub session"
        );
        let garbage = ViewState::from_value(Some(&json!({"page": 42, "model": "nope"})));
        assert_eq!(garbage.page, Page::Hub, "unknown page names fall back");
    }

    #[test]
    fn page_names_round_trip() {
        assert_eq!(page_from_name(Some(page_name(Page::About))), Page::About);
        assert_eq!(page_from_name(Some(page_name(Page::Hub))), Page::Hub);
        assert_eq!(page_name(Page::Hub), "hub");
        assert_eq!(page_name(Page::About), "about");
    }

    #[test]
    fn view_data_is_a_components_v2_payload_without_legacy_content() {
        let data = view_data(&SettingsModel::default(), &NavTargets::Fallback);
        assert_eq!(data["flags"], json!(components::IS_COMPONENTS_V2));
        assert!(data.get("content").is_none(), "v2 carries no top content");
    }

    #[test]
    fn hub_container_mirrors_the_original_layout() {
        let data = view_data(&SettingsModel::default(), &NavTargets::Fallback);
        let components = data["components"].as_array().unwrap();
        assert_eq!(components.len(), 2, "container plus the About row");
        let about = &components[1]["components"][0];
        assert_eq!(about["custom_id"], json!(CUSTOM_ID_ABOUT));
        assert_eq!(about["label"], json!("🛈 About"));

        let children = components[0]["components"].as_array().unwrap();
        assert_eq!(
            children.len(),
            6,
            "header, info, buttons, info, select, nav"
        );
        assert_eq!(children[0]["content"], json!("-# **Settings**"));
        assert!(
            children[1]["content"]
                .as_str()
                .unwrap()
                .starts_with("### Configure Feature Settings")
        );
        assert!(
            children[3]["content"]
                .as_str()
                .unwrap()
                .starts_with("### Enable or Disable Features")
        );
    }

    #[test]
    fn config_buttons_render_one_per_feature() {
        let data = view_data(&SettingsModel::default(), &NavTargets::Fallback);
        let buttons = data["components"][0]["components"][2]["components"]
            .as_array()
            .unwrap();
        let labels = ["Feeds", "Voice", "Welcome"];
        for (button, label) in buttons.iter().zip(labels) {
            assert_eq!(button["type"], json!(2));
            assert_eq!(button["style"], json!(2), "secondary like the original");
            assert_eq!(button["label"], json!(label));
            assert_eq!(
                button["custom_id"],
                json!(format!("{CUSTOM_ID_CONFIG_PREFIX}{}", label.to_lowercase()))
            );
        }
    }

    #[test]
    fn toggle_select_labels_mirror_the_model() {
        let model = SettingsModel {
            feeds_enabled: true,
            voice_enabled: false,
            welcome_enabled: true,
        };
        let data = view_data(&model, &NavTargets::Fallback);
        let options = data["components"][0]["components"][4]["components"][0]["options"]
            .as_array()
            .unwrap();
        assert_eq!(options[0]["label"], json!("✅ Feeds"));
        assert_eq!(options[1]["label"], json!("⬜ Voice"));
        assert_eq!(options[2]["label"], json!("✅ Welcome"));
        for (option, label) in options.iter().zip(["Feeds", "Voice", "Welcome"]) {
            assert_eq!(option["value"], json!(label));
        }
    }

    #[test]
    fn nav_row_follows_the_toggle_select_inside_the_container() {
        let data = view_data(&SettingsModel::default(), &NavTargets::Fallback);
        let children = data["components"][0]["components"].as_array().unwrap();
        let nav = &children[5]["components"];
        assert_eq!(nav.as_array().unwrap().len(), 1);
        assert_eq!(nav[0]["custom_id"], json!("settings:open:hello"));
    }

    #[test]
    fn discovered_nav_renders_one_button_per_running_plugin() {
        let nav = NavTargets::Discovered(vec!["hello".into(), "feed".into()]);
        let data = view_data(&SettingsModel::default(), &nav);
        let children = data["components"][0]["components"].as_array().unwrap();
        let buttons = children[5]["components"].as_array().unwrap();
        assert_eq!(buttons.len(), 2, "one button per running plugin");
        assert_eq!(buttons[0]["custom_id"], json!("settings:open:hello"));
        assert_eq!(buttons[1]["custom_id"], json!("settings:open:feed"));
    }

    #[test]
    fn an_empty_discovery_renders_no_nav_row() {
        let nav = NavTargets::Discovered(Vec::new());
        let data = view_data(&SettingsModel::default(), &nav);
        let children = data["components"][0]["components"].as_array().unwrap();
        assert_eq!(
            children.len(),
            5,
            "the nav row is dropped when no plugin runs"
        );
    }

    #[test]
    fn about_panel_renders_section_license_row_and_back_button() {
        let data = about_view();
        assert_eq!(data["flags"], json!(components::IS_COMPONENTS_V2));
        let components = data["components"].as_array().unwrap();
        assert_eq!(components.len(), 2, "container plus the Back row");
        assert_eq!(
            components[1]["components"][0]["custom_id"],
            json!(CUSTOM_ID_ABOUT_BACK)
        );

        let children = components[0]["components"].as_array().unwrap();
        assert_eq!(children.len(), 2, "section plus the license row");
        let section = &children[0];
        assert_eq!(section["type"], json!(9));
        assert!(
            section["components"][0]["content"]
                .as_str()
                .unwrap()
                .contains("Settings > About")
        );
        assert_eq!(section["accessory"]["type"], json!(2));
        assert_eq!(section["accessory"]["style"], json!(5));
        assert_eq!(children[1]["components"][0]["label"], json!("License"));
    }

    #[test]
    fn page_swap_routes_about_back_and_config_stubs() {
        assert_eq!(
            page_swap(Some(CUSTOM_ID_ABOUT), Page::Hub),
            Some(Page::About)
        );
        assert_eq!(
            page_swap(Some(CUSTOM_ID_ABOUT_BACK), Page::About),
            Some(Page::Hub)
        );
        assert_eq!(
            page_swap(Some("settings:config:feeds"), Page::About),
            Some(Page::About),
            "a stub keeps the current page"
        );
        assert_eq!(
            page_swap(Some("settings:config:"), Page::Hub),
            None,
            "an empty target is not a config click"
        );
        assert_eq!(page_swap(Some(CUSTOM_ID_TOGGLE), Page::Hub), None);
        assert_eq!(page_swap(None, Page::Hub), None);
    }

    #[test]
    fn toggle_msg_for_maps_feature_labels() {
        assert_eq!(toggle_msg_for("Feeds"), Some(SettingsMsg::Feeds));
        assert_eq!(toggle_msg_for("Voice"), Some(SettingsMsg::Voice));
        assert_eq!(toggle_msg_for("Welcome"), Some(SettingsMsg::Welcome));
        assert_eq!(toggle_msg_for("nope"), None);
    }

    #[test]
    fn parse_list_plugins_extracts_the_running_names_except_self() {
        assert_eq!(
            parse_list_plugins(Some(&json!({ "plugins": ["settings", "hello"] }))),
            Some(vec!["hello".into()])
        );
    }

    #[test]
    fn parse_list_plugins_drops_a_self_only_list_to_empty() {
        assert_eq!(
            parse_list_plugins(Some(&json!({ "plugins": ["settings"] }))),
            Some(Vec::new())
        );
    }

    #[test]
    fn parse_list_plugins_accepts_an_empty_list() {
        assert_eq!(
            parse_list_plugins(Some(&json!({ "plugins": [] }))),
            Some(Vec::new())
        );
    }

    #[test]
    fn parse_list_plugins_fails_on_a_malformed_payload() {
        assert_eq!(parse_list_plugins(None), None);
        assert_eq!(parse_list_plugins(Some(&json!({}))), None);
        assert_eq!(
            parse_list_plugins(Some(&json!({ "plugins": [1, 2] }))),
            None
        );
        assert_eq!(
            parse_list_plugins(Some(&json!({ "plugins": "nope" }))),
            None
        );
    }

    #[test]
    fn open_view_args_carry_channel_plugin_and_command() {
        let args = open_view_args(987_654_321, "hello");
        assert_eq!(args["channel_id"], json!(987_654_321));
        assert_eq!(args["plugin"], json!("hello"));
        assert_eq!(args["command"], json!("hello"));
        assert_eq!(args["args"], json!({}));
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
