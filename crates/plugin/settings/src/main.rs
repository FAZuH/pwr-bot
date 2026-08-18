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
//! - answers `invoke` of the `settings` command with the settings entry view,
//!   loading the persisted model from `host.kv.get` (`namespace='settings'`)
//!   on first open and applying the default model when the key is unset;
//! - answers `view.interact` (`settings:feeds` / `settings:voice` /
//!   `settings:welcome`) by toggling the model via the settings update logic
//!   and persisting it through `host.kv.set` before replying;
//! - a `settings:open:<plugin>` nav click issues `host.open_view` for the
//!   target plugin (the settings hub's promise: navigate to any panel),
//!   answering the interaction with the settings envelope again;
//! - the nav row is built at runtime from the host's running plugins
//!   (`host.list_plugins`); a host without that cap — or a manager-less
//!   spawn — falls back to the single default target;
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
/// what to do with the host's resp once it arrives.
#[derive(Debug, Clone, Copy)]
enum Pending {
    /// The `host.kv.get` issued to load the model before the first render.
    Load(u64),
    /// The `host.kv.set` issued to persist the model after a mutation.
    Save(u64),
    /// The `host.open_view` issued to open a target plugin's panel.
    OpenView(u64),
    /// The `host.list_plugins` issued to discover the nav row's targets.
    ListPlugins(u64),
}

impl Pending {
    /// The host op this pending kind belongs to. The op string lives here so
    /// it stays paired with the kind that resolves its resp.
    fn op(self) -> &'static str {
        match self {
            Pending::Load(_) => "host.kv.get",
            Pending::Save(_) => "host.kv.set",
            Pending::OpenView(_) => "host.open_view",
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
/// current state in the label and button style (green when enabled), plus a
/// nav row opening each discovered target plugin's panel.
fn view_data(model: &SettingsModel, nav: &NavTargets) -> Value {
    let feeds = toggle_button(CUSTOM_ID_FEEDS, "Feeds", model.feeds_enabled);
    let voice = toggle_button(CUSTOM_ID_VOICE, "Voice", model.voice_enabled);
    let welcome = toggle_button(CUSTOM_ID_WELCOME, "Welcome", model.welcome_enabled);
    let open = match nav {
        NavTargets::Fallback => nav_buttons(&[NAV_TARGET_DEFAULT]),
        NavTargets::Discovered(targets) => {
            let refs: Vec<&str> = targets.iter().map(String::as_str).collect();
            nav_buttons(&refs)
        }
    };
    let mut rows = vec![components::action_row([feeds, voice, welcome])];
    if !open.is_empty() {
        rows.push(components::action_row(open));
    }
    components::view_data("-# **Settings**", rows)
}

/// The nav row's buttons, one per declared target: empty when no target is
/// declared, so a plugin the hub cannot open never renders a dead button.
fn nav_buttons(targets: &[&str]) -> Vec<Value> {
    targets.iter().map(|target| nav_button(target)).collect()
}

/// One feature toggle button: the label shows the state and the style follows
/// it (style 3 success when enabled, style 2 secondary when disabled).
fn toggle_button(custom_id: &str, label: &str, enabled: bool) -> Value {
    let state = if enabled { "✅" } else { "⬜" };
    let style = if enabled { 3 } else { 2 };
    components::button_with_style(custom_id, format!("{state} {label}"), style)
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
/// the opaque view state the host stores and hands back on interactions.
fn envelope(model: &SettingsModel, nav: &NavTargets) -> Value {
    json!({
        "data": view_data(model, nav),
        "ephemeral": false,
        "view": model.to_value(),
    })
}

/// Parses a `host.list_plugins` resp into the running plugin names. `None`
/// on a missing, non-object, or malformed payload, so the caller falls back
/// to [`NavTargets::Fallback`].
fn parse_list_plugins(data: Option<&Value>) -> Option<Vec<String>> {
    let plugins = data?.get("plugins")?.as_array()?;
    let names: Vec<&str> = plugins.iter().map(Value::as_str).collect::<Option<_>>()?;
    Some(names.into_iter().map(str::to_string).collect())
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
/// call is logged, not fatal — the envelope still renders with the current
/// model. Returns whether the write succeeded.
fn answer_envelope(
    out: &mut impl Write,
    invoke_id: u64,
    ok: bool,
    error: Option<WireError>,
    pending_kind: Pending,
    model: Option<SettingsModel>,
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
    let resp = Msg::resp_ok(invoke_id, Some(envelope(&model.unwrap_or_default(), nav)));
    write_msg(out, &resp).is_ok()
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
    let mut nav = NavTargets::Fallback;
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
                // with the entry view; the first invoke loads from KV first.
                if (op.as_str(), cmd.as_deref()) == ("invoke", Some(PLUGIN_NAME)) && model.is_some()
                {
                    let resp = Msg::resp_ok(id, Some(envelope(&model.unwrap_or_default(), &nav)));
                    if write_msg(&mut out, &resp).is_err() {
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
                                Pending::OpenView(id),
                                open_view_args(channel_id, target),
                            ))
                        } else {
                            let msg = match custom_id {
                                Some(CUSTOM_ID_FEEDS) => Some(SettingsMsg::ToggleFeeds),
                                Some(CUSTOM_ID_VOICE) => Some(SettingsMsg::ToggleVoice),
                                Some(CUSTOM_ID_WELCOME) => Some(SettingsMsg::ToggleWelcome),
                                _ => None,
                            };
                            let Some(msg) = msg else {
                                if !reply_err(
                                    &mut out,
                                    id,
                                    "UnknownAction",
                                    format!("unknown custom_id: {custom_id:?}"),
                                ) {
                                    return ExitCode::FAILURE;
                                }
                                continue;
                            };
                            let mut current = model.unwrap_or_default();
                            update(msg, &mut current);
                            model = Some(current);
                            Some(HostCall::new(Pending::Save(id), kv_set_args(&current)))
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
                        // before the first render.
                        match parse_list_plugins(data.as_ref()) {
                            Some(targets) => nav = NavTargets::Discovered(targets),
                            None => {
                                eprintln!("host.list_plugins failed: {error:?}");
                                nav = NavTargets::Fallback;
                            }
                        }
                        let call = HostCall::new(
                            Pending::Save(invoke_id),
                            kv_set_args(&model.unwrap_or_default()),
                        );
                        if !issue_host_call(&mut out, &mut pending, &mut next_call_id, call) {
                            return ExitCode::FAILURE;
                        }
                    }
                    Pending::Save(invoke_id) | Pending::OpenView(invoke_id) => {
                        if !answer_envelope(
                            &mut out,
                            invoke_id,
                            ok,
                            error,
                            pending_kind,
                            model,
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
        let envelope = envelope(&SettingsModel::default(), &NavTargets::Fallback);
        assert!(envelope.get("data").is_some());
        assert_eq!(envelope["ephemeral"], false);
        assert!(envelope.get("view").is_some());
    }

    #[test]
    fn view_data_renders_one_toggle_per_feature() {
        let data = view_data(&SettingsModel::default(), &NavTargets::Fallback);
        assert_eq!(data["content"], "-# **Settings**");
        let buttons = &data["components"][0]["components"];
        assert_eq!(buttons.as_array().unwrap().len(), 3);
        assert_eq!(buttons[0]["custom_id"], CUSTOM_ID_FEEDS);
        assert_eq!(buttons[1]["custom_id"], CUSTOM_ID_VOICE);
        assert_eq!(buttons[2]["custom_id"], CUSTOM_ID_WELCOME);
    }

    #[test]
    fn view_data_fallback_renders_a_nav_row_after_the_toggles() {
        let data = view_data(&SettingsModel::default(), &NavTargets::Fallback);
        let rows = data["components"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "toggle row plus nav row");
        let nav = &rows[1]["components"];
        let buttons = nav.as_array().unwrap();
        assert_eq!(buttons.len(), 1, "the default target");
        assert_eq!(
            buttons[0]["custom_id"],
            json!(format!("{CUSTOM_ID_OPEN_PREFIX}{NAV_TARGET_DEFAULT}"))
        );
        assert_eq!(buttons[0]["style"], json!(1));
    }

    #[test]
    fn view_data_discovered_renders_one_button_per_running_plugin() {
        let nav = NavTargets::Discovered(vec!["hello".into(), "feed".into()]);
        let data = view_data(&SettingsModel::default(), &nav);
        let rows = data["components"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "toggle row plus nav row");
        let buttons = rows[1]["components"].as_array().unwrap();
        assert_eq!(buttons.len(), 2, "one nav button per running plugin");
        assert_eq!(buttons[0]["custom_id"], json!("settings:open:hello"));
        assert_eq!(buttons[1]["custom_id"], json!("settings:open:feed"));
    }

    #[test]
    fn view_data_discovered_empty_renders_no_nav_row() {
        let nav = NavTargets::Discovered(Vec::new());
        let data = view_data(&SettingsModel::default(), &nav);
        let rows = data["components"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "no nav row when no plugin is running");
    }

    #[test]
    fn nav_buttons_render_one_button_per_declared_target() {
        assert!(nav_buttons(&[]).is_empty(), "no targets, no nav row");
        let buttons = nav_buttons(&["hello", "voice"]);
        assert_eq!(buttons.len(), 2);
        assert_eq!(buttons[0]["custom_id"], json!("settings:open:hello"));
        assert_eq!(buttons[0]["label"], json!("Open hello"));
        assert_eq!(buttons[1]["custom_id"], json!("settings:open:voice"));
    }

    #[test]
    fn parse_list_plugins_extracts_the_running_names() {
        assert_eq!(
            parse_list_plugins(Some(&json!({ "plugins": ["settings", "hello"] }))),
            Some(vec!["settings".into(), "hello".into()])
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
    fn enabled_feature_uses_the_success_style() {
        let model = SettingsModel {
            feeds_enabled: true,
            voice_enabled: false,
            welcome_enabled: false,
        };
        let buttons = &view_data(&model, &NavTargets::Fallback)["components"][0]["components"];
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
