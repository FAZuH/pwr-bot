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
//! - `settings:about` issues `host.stats` and renders the plugin-side About
//!   panel with the live values (formatted like `/about`'s Stats section);
//!   a failed op renders the fallback copy — `settings:about:back` returns
//!   to the hub, neither touching the model;
//! - every per-feature config button rides the nav id
//!   (`settings:open:feed-settings`, `settings:open:voice-settings`,
//!   `settings:open:welcome-settings`) and opens the migrated panel plugin
//!   (ADR-0009);
//! - a `settings:open:<plugin>` nav click issues `host.open_view` for the
//!   target plugin (the settings hub's promise: navigate to any panel),
//!   forwarding the source interaction's `guild_id` in the invoke args so
//!   panel plugins can key their settings, and answering the interaction
//!   with the current envelope again. A panel's own Back/About handoff
//!   names the page this hub lands on through the same args;
//! - the nav row is built at runtime from the host's running plugins
//!   (`host.list_plugins`), minus the settings plugin itself and the three
//!   panel plugins the config buttons already open; a host
//!   without that cap — or a manager-less spawn — falls back to the
//!   single default target;
//! - every view reply is the full envelope `{"data", "ephemeral", "view"}`
//!   the interaction engine renders verbatim;
//! - treats `event` (e.g. `view.timeout`) as one-way, never answering it;
//! - answers `ping` with `pong`, tolerates the host's hello ack silently,
//!   and exits 0 on `bye` and on EOF.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::BufRead;
use std::io::Write;
use std::process::ExitCode;

use pwr_ext::component;
use pwr_ext::view;
use pwr_ext::view_support::ButtonStyle;
use pwr_ext::view_support::CreateActionRow;
use pwr_ext::view_support::CreateButton;
use pwr_ext::view_support::CreateContainerComponent;
use pwr_ext::view_support::CreateSelectMenu;
use pwr_ext::view_support::CreateSelectMenuKind;
use pwr_ext::view_support::CreateSelectMenuOption;
use pwr_ext::view_support::check_select_menu_options;
use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::CommandDef;
use pwr_plugin_protocol::HostStats;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::WireError;
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
const CUSTOM_ID_TOGGLE: &str = "settings:toggle";
const CUSTOM_ID_ABOUT: &str = "settings:about";
const CUSTOM_ID_ABOUT_BACK: &str = "settings:about:back";

/// The configurable features, in the original hub's order: the label is both
/// the button text and the select option value (as in the monolith UI), the
/// message is the toggle it applies, and the target is the panel plugin the
/// button opens through the nav id (`settings:open:<target>`).
const FEATURES: [(&str, SettingsMsg, &str); 3] = [
    ("Feeds", SettingsMsg::Feeds, "feed-settings"),
    ("Voice", SettingsMsg::Voice, "voice-settings"),
    ("Welcome", SettingsMsg::Welcome, "welcome-settings"),
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
/// list renders no nav row at all (the #128 gate), and the [`FEATURES`] panel
/// targets are skipped at render (their buttons already ride the same ids).
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
    /// The page a panel asked the hub to open on rides the chain: the
    /// panel's Back/About handoff names it through its `page` invoke arg.
    Load(u64, Page),
    /// The `host.kv.set` issued to persist a toggled session model.
    Save(u64, ViewState),
    /// The `host.open_view` issued to open a target plugin's panel.
    OpenView(u64, ViewState),
    /// The `host.list_plugins` issued to discover the nav row's targets.
    ListPlugins(u64, Page),
    /// The `host.stats` issued to render the About panel with live values.
    Stats(u64, ViewState),
}

impl Pending {
    /// The host op this pending kind belongs to. The op string lives here so
    /// it stays paired with the kind that resolves its resp.
    fn op(self) -> &'static str {
        match self {
            Pending::Load(..) => "host.kv.get",
            Pending::Save(..) => "host.kv.set",
            Pending::OpenView(..) => "host.open_view",
            Pending::ListPlugins(..) => "host.list_plugins",
            Pending::Stats(..) => "host.stats",
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
        .find(|(name, _, _)| *name == label)
        .map(|(_, msg, _)| *msg)
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

/// About panel copy: with live stats the monolith's Stats + Info sections,
/// the values fed by `host.stats` and formatted exactly like `/about`; with
/// none, the Info section only — the graceful fallback when the op errors or
/// its payload is malformed (the plugin's own version closes the footer).
fn about_copy(stats: Option<&HostStats>) -> String {
    match stats {
        Some(stats) => format!(
            concat!(
                "-# **Settings > About**\n",
                "## pwr-bot\n",
                "### Stats\n",
                "- **Uptime**: {}\n",
                "- **Servers**: {}\n",
                "- **Users**: {}\n",
                "- **Commands**: {}\n",
                "- **Latency**: {}ms\n",
                "- **Memory**: {:.1} MB\n",
                "### Info\n",
                "- **Author**: [FAZuH](https://github.com/FAZuH)\n",
                "- **Source**: [GitHub](https://github.com/FAZuH/pwr-bot)\n",
                "- **License**: [MIT](https://github.com/FAZuH/pwr-bot/blob/main/LICENSE)\n",
                "Copyright © FAZuH — v{}"
            ),
            format_uptime(stats.uptime_secs),
            format_number(stats.guild_count),
            format_number(stats.user_count),
            stats.command_count,
            stats.latency_ms,
            stats.memory_mb,
            stats.version,
        ),
        None => format!(
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
        ),
    }
}

/// Formats an uptime in seconds the way the monolith's `/about` does: the
/// coarsest nonzero unit leads, days keep hours and minutes.
fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    if days > 0 {
        format!("{days} days, {hours} hours, {minutes} minutes")
    } else if hours > 0 {
        format!("{hours} hours, {minutes} minutes")
    } else {
        format!("{minutes} minutes")
    }
}

/// Formats a count with k/M suffixes for readability, matching `/about`.
fn format_number(num: u64) -> String {
    if num >= 1_000_000 {
        format!("{:.1}M", num as f64 / 1_000_000.0)
    } else if num >= 1_000 {
        format!("{:.1}k", num as f64 / 1_000.0)
    } else {
        num.to_string()
    }
}

/// Renders the settings hub as Components V2, mirroring the original monolith
/// layout: one container with the header, both info sections, the per-feature
/// button row, and the toggle select whose ✅/⬜ labels mirror the model; the
/// discovered-plugins nav row follows inside the container, and the 🛈 About
/// button sits outside it.
///
/// The message is one `view!` literal whose runtime pieces are spliced at
/// their pinned positions: the config buttons, the whole toggle row, and the
/// nav rows. [`FEATURES`] stays the single source of truth — the
/// config buttons and toggle options derive from it rather than being forked
/// into a literal view. The nav rows are spliced from a [`Vec`], so a plugin
/// list with nothing to open drops the row entirely instead of rendering dead
/// buttons; more than five discovered targets spill into further rows. The
/// toggle select is the one row built at runtime rather than as a literal:
/// the grammar's select-menu options arm is
/// literal-only, so the options mirror the model through the typed builders
/// and pass through [`check_select_menu_options`] to keep the law. Every
/// spliced parent that carries a child rule (the config and nav button rows)
/// is guarded by a macro-emitted `check_*` call over its combined children —
/// a violation panics loudly instead of silently rendering an invalid wire,
/// the failure mode the old `components::action_row` (no law) had.
fn view_data(model: &SettingsModel, nav: &NavTargets) -> Value {
    let config_buttons: Vec<CreateButton<'static>> = FEATURES
        .iter()
        .map(|(label, _, target)| {
            CreateButton::new(format!("{CUSTOM_ID_OPEN_PREFIX}{target}"))
                .label(*label)
                .style(ButtonStyle::Secondary)
        })
        .collect();
    let toggle_options: Vec<CreateSelectMenuOption<'static>> = FEATURES
        .iter()
        .map(|(label, msg, _)| {
            let emoji = if feature_enabled(model, *msg) {
                "✅"
            } else {
                "⬜"
            };
            CreateSelectMenuOption::new(format!("{emoji} {label}"), *label)
        })
        .collect();
    check_select_menu_options(&toggle_options).expect("toggle options obey the select-menu law");
    let toggle_row =
        CreateContainerComponent::ActionRow(CreateActionRow::select_menu(CreateSelectMenu::new(
            CUSTOM_ID_TOGGLE,
            CreateSelectMenuKind::String {
                options: Cow::Owned(toggle_options),
            },
        )));
    let nav_rows = nav_rows(nav);
    let message = view! {
        components_v2 {
            container {
                text_display { content: "-# **Settings**" }
                text_display { content: CONFIGURE_INFO }
                action_row { { config_buttons } }
                text_display { content: TOGGLE_INFO }
                { Some(toggle_row) }
                { nav_rows }
            }
            action_row {
                button {
                    custom_id: CUSTOM_ID_ABOUT,
                    label: "🛈 About",
                    style: ButtonStyle::Secondary
                }
            }
        }
    }
    .expect("spliced view obeys the component laws");
    serde_json::to_value(message).expect("settings hub view is serializable")
}

/// Renders the plugin-side About panel: a container holding a section with
/// the About copy (a Source Code link button as its accessory) plus the
/// License link row, and the ❮ Back button outside. The copy is live when the
/// plugin holds a [`HostStats`] snapshot, the fallback copy otherwise.
fn about_view(stats: Option<&HostStats>) -> Value {
    let copy = about_copy(stats);
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

/// The nav rows as full container children, one `action_row` per chunk of at
/// most five targets: a [`NavTargets::Fallback`] renders a single row holding
/// the one [`NAV_TARGET_DEFAULT`]; an empty [`NavTargets::Discovered`] renders
/// no row at all (the hub never shows a dead button); a non-empty list renders
/// one `Open <target>` button per target, chunked so the one-action-row
/// five-button law is never violated — [`Vec::chunks`] never yields an empty
/// chunk, so no row is drawn without a button. Discovered targets that a
/// config button already opens (the [`FEATURES`] panels) are skipped: a
/// second button with the same `settings:open:<target>` id would make Discord
/// reject the whole message. The container law caps a
/// container at 40 children, so the edge is roughly 175 targets (5 literal
/// children + `ceil(n/5)` rows); past that it fails loudly rather than
/// silently, by design. Each row is authored with `component!` (D2) and
/// wrapped explicitly in [`CreateContainerComponent::ActionRow`]; the button
/// list is spliced in, so the macro-emitted runtime law check guards the
/// one-action-row button cap (which `chunks(5)` makes unfireable).
fn nav_rows(nav: &NavTargets) -> Vec<CreateContainerComponent<'static>> {
    let targets: Vec<&str> = match nav {
        NavTargets::Fallback => vec![NAV_TARGET_DEFAULT],
        NavTargets::Discovered(targets) => targets
            .iter()
            .map(String::as_str)
            .filter(|target| !FEATURES.iter().any(|(_, _, feature)| feature == target))
            .collect(),
    };
    targets
        .chunks(5)
        .map(|chunk| {
            let buttons: Vec<CreateButton<'static>> =
                chunk.iter().map(|&target| nav_button(target)).collect();
            CreateContainerComponent::ActionRow(
                component! {
                    action_row {
                        { buttons }
                    }
                }
                .expect("spliced row obeys the button law"),
            )
        })
        .collect()
}

/// The nav button opening another plugin's panel: the target name rides in
/// the custom id (`settings:open:<target>`), and the label names the target.
fn nav_button(target: &str) -> CreateButton<'static> {
    CreateButton::new(format!("{CUSTOM_ID_OPEN_PREFIX}{target}")).label(format!("Open {target}"))
}

/// The render inputs every envelope draw reads beyond the session state:
/// the discovered nav targets and the last known stats snapshot. Both are
/// process-global caches, threaded explicitly so tests can pin them.
struct RenderCtx<'a> {
    nav: &'a NavTargets,
    stats: Option<&'a HostStats>,
}

/// The full envelope a view reply carries: raw message data, visibility, and
/// the session state the host stores per message and hands back on
/// interactions. The last known `host.stats` snapshot feeds the About page;
/// `None` renders its fallback copy.
fn envelope(state: &ViewState, ctx: &RenderCtx<'_>) -> Value {
    let data = match state.page {
        Page::Hub => view_data(&state.model, ctx.nav),
        Page::About => about_view(ctx.stats),
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
    ctx: &RenderCtx<'_>,
) -> bool {
    let resp = Msg::resp_ok(invoke_id, Some(envelope(state, ctx)));
    write_msg(out, &resp).is_ok()
}

/// The page a click needing no host call lands on: Back returns to the hub.
/// About is not here — it needs a `host.stats` call first. `None` when the
/// custom id is neither.
fn page_swap(custom_id: Option<&str>) -> Option<Page> {
    match custom_id {
        Some(CUSTOM_ID_ABOUT_BACK) => Some(Page::Hub),
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
/// plugin and the command, and the source guild's id when the interaction
/// carried one — the panel plugins key their settings by guild.
fn open_view_args(channel_id: u64, guild_id: Option<u64>, plugin: &str) -> Value {
    let mut invoke_args = serde_json::Map::new();
    if let Some(guild_id) = guild_id {
        invoke_args.insert("guild_id".into(), json!(guild_id));
    }
    json!({
        "channel_id": channel_id,
        "plugin": plugin,
        "command": plugin,
        "args": invoke_args,
    })
}

/// Reads a Discord id from a wire value: a number, or the string form
/// serenity's ids serialize to.
fn id_as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

/// The `host.stats` call args: the op takes none.
fn stats_args() -> Value {
    json!({})
}

/// Parses a `host.stats` resp payload; `None` on a missing or malformed
/// payload, so the caller keeps its last known snapshot.
fn parse_stats(data: Option<&Value>) -> Option<HostStats> {
    serde_json::from_value(data?.clone()).ok()
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
    ctx: &RenderCtx<'_>,
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
    reply_envelope(out, invoke_id, state, ctx)
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
    // The last stats snapshot the host served, fed to the About page. It is
    // process-global like the model and nav caches (stats are host-wide, not
    // session-wide); a failed refresh keeps the last snapshot, and until the
    // first one arrives the About page renders its fallback copy.
    let mut stats: Option<HostStats> = None;
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
            "host.stats".into(),
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
                // sessions their messages carry, and a panel's handoff names
                // the page to land on (its `page` arg).
                if (op.as_str(), cmd.as_deref()) == ("invoke", Some(PLUGIN_NAME)) && model.is_some()
                {
                    let page = page_from_name(
                        args.as_ref()
                            .and_then(|a| a.get("page"))
                            .and_then(Value::as_str),
                    );
                    let state = ViewState {
                        model: model.unwrap_or_default(),
                        page,
                    };
                    if !reply_envelope(
                        &mut out,
                        id,
                        &state,
                        &RenderCtx {
                            nav: &nav,
                            stats: stats.as_ref(),
                        },
                    ) {
                        return ExitCode::FAILURE;
                    }
                    continue;
                }
                let host_call = match (op.as_str(), cmd.as_deref()) {
                    ("invoke", Some(PLUGIN_NAME)) => {
                        let page = page_from_name(
                            args.as_ref()
                                .and_then(|a| a.get("page"))
                                .and_then(Value::as_str),
                        );
                        Some(HostCall::new(Pending::Load(id, page), kv_get_args()))
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
                                .and_then(id_as_u64)
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
                            let guild_id = args
                                .as_ref()
                                .and_then(|a| a.get("guild_id"))
                                .and_then(id_as_u64);
                            Some(HostCall::new(
                                Pending::OpenView(id, session),
                                open_view_args(channel_id, guild_id, target),
                            ))
                        } else if custom_id == Some(CUSTOM_ID_ABOUT) {
                            // About opens with live stats: one `host.stats`
                            // round trip, then the resp renders the panel —
                            // live values on success, the fallback copy when
                            // the op errors. The session swaps to the About
                            // page either way.
                            Some(HostCall::new(Pending::Stats(id, session), stats_args()))
                        } else if let Some(next_page) = page_swap(custom_id) {
                            // The only host-call-free page swap left: Back
                            // from About to the hub. Every Configure button
                            // is a nav id handled above.
                            let state = ViewState {
                                model: session.model,
                                page: next_page,
                            };
                            if !reply_envelope(
                                &mut out,
                                id,
                                &state,
                                &RenderCtx {
                                    nav: &nav,
                                    stats: stats.as_ref(),
                                },
                            ) {
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
                    Pending::Load(invoke_id, page) => {
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
                        let call = HostCall::new(Pending::ListPlugins(invoke_id, page), json!({}));
                        if !issue_host_call(&mut out, &mut pending, &mut next_call_id, call) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::ListPlugins(invoke_id, page) => {
                        // The running plugin names, or the default target when
                        // discovery failed; then persist the panel state
                        // before the first render. The session lands on the
                        // page the handoff asked for, the hub page by default.
                        match parse_list_plugins(data.as_ref()) {
                            Some(targets) => nav = NavTargets::Discovered(targets),
                            None => {
                                eprintln!("host.list_plugins failed: {error:?}");
                                nav = NavTargets::Fallback;
                            }
                        }
                        let state = ViewState {
                            model: model.unwrap_or_default(),
                            page,
                        };
                        let call = HostCall::new(
                            Pending::Save(invoke_id, state),
                            kv_set_args(&state.model),
                        );
                        if !issue_host_call(&mut out, &mut pending, &mut next_call_id, call) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::Stats(invoke_id, session) => {
                        // The panel renders on the About page either way: a
                        // successful gather refreshes the snapshot, a failure
                        // (typed error or malformed payload) keeps the last
                        // one — until the first arrival the fallback copy
                        // shows.
                        let fresh = if ok { parse_stats(data.as_ref()) } else { None };
                        match fresh {
                            Some(fresh) => stats = Some(fresh),
                            None => eprintln!("host.stats failed: {error:?}"),
                        }
                        let state = ViewState {
                            model: session.model,
                            page: Page::About,
                        };
                        if !reply_envelope(
                            &mut out,
                            invoke_id,
                            &state,
                            &RenderCtx {
                                nav: &nav,
                                stats: stats.as_ref(),
                            },
                        ) {
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
                            &RenderCtx {
                                nav: &nav,
                                stats: stats.as_ref(),
                            },
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
    use pwr_poise_components as components;

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
        let envelope = envelope(
            &state,
            &RenderCtx {
                nav: &NavTargets::Fallback,
                stats: None,
            },
        );
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
        }
        // All three features migrated (ADR-0009): their buttons open the
        // panel plugins.
        assert_eq!(
            buttons[0]["custom_id"],
            json!("settings:open:feed-settings")
        );
        assert_eq!(
            buttons[1]["custom_id"],
            json!("settings:open:voice-settings")
        );
        assert_eq!(
            buttons[2]["custom_id"],
            json!("settings:open:welcome-settings")
        );
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
    fn panel_only_discovery_renders_no_nav_row() {
        // The three panel plugins are the config buttons' own targets: nav
        // buttons for them would duplicate the `settings:open:*` ids and
        // Discord would reject the whole message
        // (COMPONENT_CUSTOM_ID_DUPLICATED) — with only them running, the
        // #128 gate drops the row entirely.
        let nav = NavTargets::Discovered(
            FEATURES
                .iter()
                .map(|(_, _, target)| target.to_string())
                .collect(),
        );
        let data = view_data(&SettingsModel::default(), &nav);
        let children = data["components"][0]["components"].as_array().unwrap();
        assert_eq!(
            children.len(),
            5,
            "the nav row is dropped when only the panel plugins run"
        );
    }

    #[test]
    fn nav_row_skips_panel_targets_but_keeps_other_plugins() {
        let nav = NavTargets::Discovered(vec![
            "hello".into(),
            "feed-settings".into(),
            "voice-settings".into(),
            "welcome-settings".into(),
        ]);
        let data = view_data(&SettingsModel::default(), &nav);
        let children = data["components"][0]["components"].as_array().unwrap();
        assert_eq!(children.len(), 6, "exactly one nav row survives");
        let buttons = children[5]["components"].as_array().unwrap();
        assert_eq!(buttons.len(), 1, "only the plugin no config button opens");
        assert_eq!(buttons[0]["custom_id"], json!("settings:open:hello"));
    }

    #[test]
    fn nav_rows_chunk_six_targets_into_a_five_and_one_row() {
        // Six discovered targets spill into two rows: the first holds the
        // first five, the second the sixth — the one-action-row five-button
        // law is honored by chunking rather than panicking.
        let nav = NavTargets::Discovered(vec![
            "a".into(),
            "b".into(),
            "c".into(),
            "d".into(),
            "e".into(),
            "f".into(),
        ]);
        let data = view_data(&SettingsModel::default(), &nav);
        let children = data["components"][0]["components"].as_array().unwrap();
        assert_eq!(
            children.len(),
            7,
            "the container is the five literals plus two nav rows"
        );
        let first = children[5]["components"].as_array().unwrap();
        assert_eq!(first.len(), 5, "the first row holds the first five targets");
        for (button, target) in first.iter().zip(["a", "b", "c", "d", "e"]) {
            assert_eq!(
                button["custom_id"],
                json!(format!("{CUSTOM_ID_OPEN_PREFIX}{target}"))
            );
            assert_eq!(button["label"], json!(format!("Open {target}")));
        }
        let second = children[6]["components"].as_array().unwrap();
        assert_eq!(second.len(), 1, "the second row holds the remaining target");
        assert_eq!(second[0]["custom_id"], json!("settings:open:f"));
        assert_eq!(second[0]["label"], json!("Open f"));
    }

    #[test]
    fn nav_rows_chunk_eleven_targets_into_three_rows() {
        // Eleven targets spill into three rows of 5/5/1, preserving order and
        // the per-button ids and labels throughout.
        let targets: Vec<String> = (0..11).map(|i| format!("t{i}")).collect();
        let nav = NavTargets::Discovered(targets);
        let data = view_data(&SettingsModel::default(), &nav);
        let children = data["components"][0]["components"].as_array().unwrap();
        assert_eq!(
            children.len(),
            8,
            "the container is the five literals plus three nav rows"
        );
        for (row_index, (expected, start)) in [(5, 0), (5, 5), (1, 10)].iter().enumerate() {
            let buttons = children[5 + row_index]["components"].as_array().unwrap();
            assert_eq!(buttons.len(), *expected);
            for (button_index, button) in buttons.iter().enumerate() {
                assert_eq!(
                    button["custom_id"],
                    json!(format!("{CUSTOM_ID_OPEN_PREFIX}t{}", start + button_index))
                );
            }
        }
    }

    #[test]
    fn about_panel_renders_section_license_row_and_back_button() {
        let data = about_view(None);
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
    fn about_panel_renders_live_stats_like_the_monolith() {
        let stats = fixture_stats();
        let data = about_view(Some(&stats));
        let content = data["components"][0]["components"][0]["components"][0]["content"]
            .as_str()
            .unwrap();
        for line in [
            "### Stats",
            "- **Uptime**: 1 days, 1 hours, 0 minutes",
            "- **Servers**: 2",
            "- **Users**: 1.5k",
            "- **Commands**: 12",
            "- **Latency**: 42ms",
            "- **Memory**: 320.0 MB",
            "- **Author**: [FAZuH](https://github.com/FAZuH)",
            "Copyright © FAZuH — v1.2.3",
        ] {
            assert!(content.contains(line), "missing {line:?} in: {content}");
        }
        assert!(
            !content.contains(env!("CARGO_PKG_VERSION")),
            "live copy shows the host's version, not the plugin's"
        );
    }

    #[test]
    fn about_panel_without_stats_renders_the_fallback_copy() {
        let data = about_view(None);
        let content = data["components"][0]["components"][0]["components"][0]["content"]
            .as_str()
            .unwrap();
        assert!(
            !content.contains("### Stats"),
            "no snapshot, no stats section: {content}"
        );
        assert!(content.contains("Copyright © FAZuH — v"), "{content}");
    }

    fn fixture_stats() -> HostStats {
        HostStats {
            version: "1.2.3".into(),
            uptime_secs: 90_000,
            guild_count: 2,
            user_count: 1_500,
            latency_ms: 42,
            command_count: 12,
            memory_mb: 320.0,
        }
    }

    #[test]
    fn format_uptime_mirrors_the_monolith() {
        assert_eq!(format_uptime(90_000), "1 days, 1 hours, 0 minutes");
        assert_eq!(format_uptime(3_600), "1 hours, 0 minutes");
        assert_eq!(format_uptime(300), "5 minutes");
    }

    #[test]
    fn format_number_mirrors_the_monolith() {
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1_500), "1.5k");
        assert_eq!(format_number(2_000_000), "2.0M");
    }

    #[test]
    fn parse_stats_accepts_a_host_snapshot() {
        let payload = serde_json::to_value(fixture_stats()).unwrap();
        assert_eq!(parse_stats(Some(&payload)), Some(fixture_stats()));
    }

    #[test]
    fn parse_stats_fails_on_missing_or_malformed_payloads() {
        assert_eq!(parse_stats(None), None);
        assert_eq!(parse_stats(Some(&json!({}))), None);
        assert_eq!(parse_stats(Some(&json!(42))), None);
        assert_eq!(
            parse_stats(Some(&json!({"version": 1, "uptime_secs": "late"}))),
            None
        );
    }

    #[test]
    fn page_swap_routes_back_only() {
        assert_eq!(page_swap(Some(CUSTOM_ID_ABOUT_BACK)), Some(Page::Hub));
        assert_eq!(
            page_swap(Some(CUSTOM_ID_ABOUT)),
            None,
            "About needs a host.stats call, so it is not a page swap"
        );
        assert_eq!(page_swap(Some(CUSTOM_ID_TOGGLE)), None);
        assert_eq!(page_swap(None), None);
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
        let args = open_view_args(987_654_321, None, "hello");
        assert_eq!(args["channel_id"], json!(987_654_321));
        assert_eq!(args["plugin"], json!("hello"));
        assert_eq!(args["command"], json!("hello"));
        assert_eq!(args["args"], json!({}), "no guild known: args stay empty");

        let args = open_view_args(1, Some(42), "feed-settings");
        assert_eq!(
            args["args"],
            json!({ "guild_id": 42 }),
            "a known guild rides the invoke args for panel plugins"
        );
    }

    #[test]
    fn id_as_u64_accepts_numbers_and_string_ids() {
        assert_eq!(id_as_u64(&json!(42)), Some(42));
        assert_eq!(
            id_as_u64(&json!("42")),
            Some(42),
            "serenity ids serialize as strings"
        );
        assert_eq!(id_as_u64(&json!("nope")), None);
        assert_eq!(id_as_u64(&json!(null)), None);
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
