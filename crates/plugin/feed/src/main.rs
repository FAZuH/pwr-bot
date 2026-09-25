//! The feed plugin owns feed tables, feed settings, feed commands, and feed
//! delivery. It speaks the pwr-bot plugin wire protocol over JSON-Lines stdio.
//!
//! Startup announces the manifest, loads host configuration, applies the
//! plugin's embedded migrations, and then serves queued calls. The feed
//! settings panel persists directly through the plugin repository. The
//! `/feed` command and its views use the same services, with batch invokes
//! sending `Msg::Progress` updates before their final response. `Back` and
//! `About` use the host's reserved `host.open_view` targets. Timeout events
//! persist the current settings model without sending a response.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::BufRead;
use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use feed::COMMAND_NAME;
use feed::FEED_BATCH_COMMAND_NAME;
use feed::FEED_LIST_COMMAND_NAME;
use feed::FEED_SETTINGS_COMMAND_NAME;
use feed::FEED_SUBSCRIBE_COMMAND_NAME;
use feed::FEED_UNSUBSCRIBE_COMMAND_NAME;
use feed::PLUGIN_NAME;
use feed::Platforms;
use feed::command;
use feed::event::EventBus;
use feed::host_client::HostResponse;
use feed::host_client::SharedOutput;
use feed::host_client::spawn_host_client;
use feed::manifest;
use feed::repo::Repository;
use feed::service::feed_subscription::FeedSubscriptionService;
use feed::subscriber::discord_dm::DiscordDmSubscriber;
use feed::subscriber::discord_guild::DiscordGuildSubscriber;
use feed::task::series_feed_publisher::SeriesFeedPublisher;
use pwr_ext::view;
use pwr_ext::view_support::ButtonStyle;
use pwr_ext::view_support::ChannelType;
use pwr_ext::view_support::CreateSelectMenuKind;
use pwr_ext::view_support::GenericChannelId;
use pwr_ext::view_support::RoleId;
use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::FeedsSettings;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::VIEW_MOVED_KIND;
use pwr_plugin_support::about_exit;
use pwr_plugin_support::back_exit;
use pwr_plugin_support::id_as_u64;
use pwr_plugin_support::reply_err;
use pwr_plugin_support::write_msg;
use serde_json::Value;
use serde_json::json;

/// Custom ids for the panel's interactive components.
const CUSTOM_ID_TOGGLE: &str = "feeds:toggle";
const CUSTOM_ID_CHANNEL: &str = "feeds:channel";
const CUSTOM_ID_SUB_ROLE: &str = "feeds:sub-role";
const CUSTOM_ID_UNSUB_ROLE: &str = "feeds:unsub-role";
const CUSTOM_ID_BACK: &str = "feeds:back";
const CUSTOM_ID_ABOUT: &str = "feeds:about";

/// Panel copy for the feed settings view.
const CHANNEL_TEXT: &str =
    "### Notification Channel\n\n> 🛈  Choose where feed updates will be posted.";
const SUB_ROLE_TEXT: &str = concat!(
    "### Subscribe Permission\n\n",
    "> 🛈  Who can add new feeds to this server. Leave empty to allow users with ",
    "\"Manage Server\" permission."
);
const UNSUB_ROLE_TEXT: &str = concat!(
    "### Unsubscribe Permission\n\n",
    "> 🛈  Who can remove feeds from this server. Leave empty to allow users with ",
    "\"Manage Server\" permission."
);

/// The feed settings model.
#[derive(Debug, Clone, PartialEq)]
struct Model {
    settings: FeedsSettings,
}

impl Model {
    fn new(settings: FeedsSettings) -> Self {
        Self { settings }
    }

    /// Whether feed notifications are enabled (defaults to enabled).
    fn is_enabled(&self) -> bool {
        self.settings.enabled.unwrap_or(true)
    }

    fn channel_id(&self) -> Option<String> {
        self.settings.channel_id.clone()
    }

    fn subscribe_role_id(&self) -> Option<String> {
        self.settings.subscribe_role_id.clone()
    }

    fn unsubscribe_role_id(&self) -> Option<String> {
        self.settings.unsubscribe_role_id.clone()
    }
}

/// Messages handled by the feed settings view.
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

/// A request to persist the current settings snapshot.
#[derive(Debug, Clone, PartialEq)]
enum Effect {
    Persist(FeedsSettings),
}

/// The pure update function — the only writer of the model. `Back`, `About`,
/// and `Expired` persist the current snapshot; every edit applies in place
/// with no effect.
fn update(msg: PanelMsg, model: &mut Model) -> Vec<Effect> {
    match msg {
        PanelMsg::ToggleEnabled => {
            let current = model.settings.enabled.unwrap_or(true);
            model.settings.enabled = Some(!current);
            Vec::new()
        }
        PanelMsg::SetChannel(id) => {
            model.settings.channel_id = id;
            Vec::new()
        }
        PanelMsg::SetSubRole(id) => {
            model.settings.subscribe_role_id = id;
            Vec::new()
        }
        PanelMsg::SetUnsubRole(id) => {
            model.settings.unsubscribe_role_id = id;
            Vec::new()
        }
        PanelMsg::Back | PanelMsg::About | PanelMsg::Expired => persist(model),
    }
}

/// The persist behavior shared by `Back` and expiry: snapshot the current
/// settings exactly once.
fn persist(model: &Model) -> Vec<Effect> {
    vec![Effect::Persist(model.settings.clone())]
}

#[derive(Debug, Clone, PartialEq)]
struct SessionState {
    guild_id: u64,
    model: Model,
}

impl SessionState {
    fn new(guild_id: u64, settings: FeedsSettings) -> Self {
        Self {
            guild_id,
            model: Model::new(settings),
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "guild_id": self.guild_id,
            "settings": self.model.settings,
        })
    }

    fn from_value(value: Option<&Value>) -> Option<Self> {
        let value = value?;
        let guild_id = value.get("guild_id").and_then(id_as_u64)?;
        let settings = value.get("settings")?;
        if settings.get("feeds").is_some() {
            return None;
        }
        let settings = serde_json::from_value(settings.clone()).ok()?;
        Some(Self::new(guild_id, settings))
    }
}

enum Pending {
    OpenSettings {
        invoke_id: u64,
        session: SessionState,
        message_id: Option<u64>,
    },
}

struct HostCall {
    pending: Pending,
    args: Value,
}

impl HostCall {
    fn open_settings(
        invoke_id: u64,
        session: SessionState,
        message_id: Option<u64>,
        args: Value,
    ) -> Self {
        Self {
            pending: Pending::OpenSettings {
                invoke_id,
                session,
                message_id,
            },
            args,
        }
    }
}

/// Renders the feed settings panel as Components V2: the status header, the
/// toggle button, the notification-channel select, the two permission-role
/// selects, and the Back/About row outside the container. Select kinds are
/// built at runtime so the current selection rides each menu's default
/// values.
fn view_data(model: &Model) -> Value {
    let is_enabled = model.is_enabled();

    let status_text = format!(
        "-# **Settings > Feeds**\n## Feed Subscription Settings\n\n> 🛈  {}",
        if is_enabled {
            match model.channel_id() {
                Some(id) => format!(
                    concat!(
                        "Feed notifications are currently **active**. ",
                        "Notifications will be sent to <#",
                        "{0}>"
                    ),
                    id
                ),
                None => concat!(
                    "Feed notifications are currently **active**, but notification channel ",
                    "is not set."
                )
                .to_string(),
            }
        } else {
            concat!(
                "Feed notifications are currently **paused**. ",
                "No notifications will be sent until it is re-enabled."
            )
            .to_string()
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
    reply_data(out, invoke_id, envelope(session))
}

fn reply_data(out: &mut impl Write, invoke_id: u64, data: Value) -> bool {
    write_msg(out, &Msg::resp_ok(invoke_id, Some(data))).is_ok()
}

struct OutputWriter(SharedOutput);

impl Write for OutputWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("stdout lock poisoned").write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().expect("stdout lock poisoned").flush()
    }
}

fn write_output(output: &SharedOutput, message: &Msg) -> std::io::Result<()> {
    let mut output = output.lock().expect("stdout lock poisoned");
    write_msg(&mut *output, message)
}

fn issue_call(
    output: &SharedOutput,
    pending: &mut HashMap<u64, Pending>,
    next_call_id: &AtomicU64,
    call: HostCall,
) -> bool {
    let call_id = next_call_id.fetch_add(1, Ordering::Relaxed);
    pending.insert(call_id, call.pending);
    let message = Msg::Call {
        id: call_id,
        op: "host.open_view".into(),
        cmd: None,
        args: Some(call.args),
    };
    write_output(output, &message).is_ok()
}

fn load_host_config(
    output: &SharedOutput,
    next_call_id: &AtomicU64,
    input: &mut impl BufRead,
) -> Result<(String, Duration, Vec<Msg>), String> {
    let call_id = next_call_id.fetch_add(1, Ordering::Relaxed);
    write_output(
        output,
        &Msg::Call {
            id: call_id,
            op: "host.get_config".into(),
            cmd: None,
            args: None,
        },
    )
    .map_err(|error| error.to_string())?;
    let mut queued = Vec::new();
    for line in input.lines() {
        let line = line.map_err(|error| error.to_string())?;
        let message: Msg = serde_json::from_str(&line).map_err(|error| error.to_string())?;
        if matches!(&message, Msg::Hello { .. }) {
            continue;
        }
        if let Msg::Resp {
            id,
            ok: true,
            data: Some(data),
            ..
        } = &message
            && *id == call_id
        {
            let db_url = data
                .get("db_url")
                .and_then(Value::as_str)
                .ok_or("host.get_config response has no db_url")?
                .to_string();
            let poll_interval = data
                .get("poll_interval")
                .and_then(Value::as_u64)
                .ok_or("host.get_config response has no poll_interval")?;
            return Ok((db_url, Duration::from_secs(poll_interval), queued));
        }
        queued.push(message);
    }
    Err("host closed before returning config".into())
}

fn publisher_enabled() -> bool {
    match std::env::var("ENABLE_FEED_PUBLISHER") {
        Ok(value) => !matches!(
            value.to_ascii_lowercase().as_str(),
            "false" | "0" | "no" | "off"
        ),
        Err(_) => true,
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let output: SharedOutput = Arc::new(Mutex::new(stdout));
    let mut out = OutputWriter(Arc::clone(&output));
    let next_call_id = Arc::new(AtomicU64::new(0));
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut input = stdin.lock();

    let hello = Msg::Hello {
        v: API_VERSION,
        name: PLUGIN_NAME.into(),
        ops: vec![
            "host.get_config".into(),
            "host.open_dm".into(),
            "host.open_view".into(),
            "host.send_message".into(),
        ],
        manifest: Some(manifest()),
    };
    if write_output(&output, &hello).is_err() {
        return ExitCode::FAILURE;
    }

    let (db_url, poll_interval, queued_messages) =
        match load_host_config(&output, &next_call_id, &mut input) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("failed to load host config: {error}");
                return ExitCode::FAILURE;
            }
        };
    let repository = match Repository::connect(&db_url).await {
        Ok(repository) => repository,
        Err(error) => {
            eprintln!("failed to connect feed storage: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = repository.migrate().await {
        eprintln!("failed to migrate feed storage: {error}");
        return ExitCode::FAILURE;
    }
    let platforms = Arc::new(Platforms::new());
    let service = Arc::new(FeedSubscriptionService::new(
        &repository,
        Arc::clone(&platforms),
    ));
    let event_bus = Arc::new(EventBus::new());
    let host_client = Arc::new(spawn_host_client(
        Arc::clone(&output),
        Arc::clone(&next_call_id),
    ));
    event_bus.register_subscriber(Arc::new(DiscordDmSubscriber::new(
        service.clone(),
        host_client.clone(),
    )));
    event_bus.register_subscriber(Arc::new(DiscordGuildSubscriber::new(
        service.clone(),
        host_client.clone(),
    )));
    let publisher = SeriesFeedPublisher::new(service.clone(), event_bus, poll_interval);
    if publisher_enabled()
        && let Err(error) = publisher.clone().start()
    {
        eprintln!("failed to start feed publisher: {error}");
        return ExitCode::FAILURE;
    }

    let mut queued_messages = queued_messages.into_iter();
    let mut lines = input.lines();
    loop {
        let msg = if let Some(message) = queued_messages.next() {
            message
        } else {
            let Some(Ok(line)) = lines.next() else {
                break;
            };
            match serde_json::from_str(&line) {
                Ok(msg) => msg,
                Err(e) => {
                    eprintln!("bad json: {e}");
                    continue;
                }
            }
        };
        match msg {
            Msg::Bye => break,
            Msg::Call { id, op, cmd, args } => {
                if op == "autocomplete" {
                    let command_name = cmd.as_deref().unwrap_or_default();
                    let response = if matches!(
                        command_name,
                        FEED_SUBSCRIBE_COMMAND_NAME | FEED_UNSUBSCRIBE_COMMAND_NAME
                    ) {
                        command::autocomplete(
                            &service,
                            &platforms,
                            command_name,
                            args.as_ref().unwrap_or(&Value::Null),
                        )
                        .await
                    } else {
                        json!({ "choices": [] })
                    };
                    if !reply_data(&mut out, id, response) {
                        return ExitCode::FAILURE;
                    }
                    continue;
                }

                let direct_result = match (op.as_str(), cmd.as_deref()) {
                    ("invoke", Some(COMMAND_NAME))
                    | ("invoke", Some(FEED_SETTINGS_COMMAND_NAME)) => Some(
                        async {
                            if matches!(
                                cmd.as_deref(),
                                Some(COMMAND_NAME) | Some(FEED_SETTINGS_COMMAND_NAME)
                            ) {
                                command::verify_settings_invocation(
                                    args.as_ref().unwrap_or(&Value::Null),
                                )?;
                            }
                            let guild_id = args
                                .as_ref()
                                .and_then(|args| args.get("guild_id"))
                                .and_then(id_as_u64)
                                .ok_or(command::CommandError::GuildOnly)?;
                            let settings = service.get_feed_settings(guild_id).await?;
                            Ok(envelope(&SessionState::new(guild_id, settings)))
                        }
                        .await,
                    ),
                    ("invoke", Some(FEED_LIST_COMMAND_NAME)) => Some(
                        command::invoke_list(&service, args.as_ref().unwrap_or(&Value::Null)).await,
                    ),
                    ("invoke", Some(FEED_SUBSCRIBE_COMMAND_NAME)) => Some(
                        command::invoke_batch(
                            &service,
                            args.as_ref().unwrap_or(&Value::Null),
                            true,
                            |data| {
                                if let Err(error) =
                                    write_output(&output, &Msg::Progress { id, data })
                                {
                                    eprintln!("failed to write feed batch progress: {error}");
                                }
                            },
                        )
                        .await,
                    ),
                    ("invoke", Some(FEED_UNSUBSCRIBE_COMMAND_NAME)) => Some(
                        command::invoke_batch(
                            &service,
                            args.as_ref().unwrap_or(&Value::Null),
                            false,
                            |data| {
                                if let Err(error) =
                                    write_output(&output, &Msg::Progress { id, data })
                                {
                                    eprintln!("failed to write feed batch progress: {error}");
                                }
                            },
                        )
                        .await,
                    ),
                    ("view.interact", Some(FEED_LIST_COMMAND_NAME)) => Some(
                        command::interact_list(&service, args.as_ref().unwrap_or(&Value::Null))
                            .await,
                    ),
                    ("view.interact", Some(FEED_BATCH_COMMAND_NAME)) => Some(
                        command::interact_batch(&service, args.as_ref().unwrap_or(&Value::Null))
                            .await,
                    ),
                    ("view.interact", Some(FEED_SUBSCRIBE_COMMAND_NAME))
                    | ("view.interact", Some(FEED_UNSUBSCRIBE_COMMAND_NAME)) => Some(
                        command::interact_feed_view(
                            &service,
                            args.as_ref().unwrap_or(&Value::Null),
                        )
                        .await,
                    ),
                    _ => None,
                };
                if let Some(result) = direct_result {
                    match result {
                        Ok(data) => {
                            if !reply_data(&mut out, id, data) {
                                return ExitCode::FAILURE;
                            }
                        }
                        Err(error) => {
                            if !reply_err(&mut out, id, "CommandError", error.to_string()) {
                                return ExitCode::FAILURE;
                            }
                        }
                    }
                    continue;
                }

                // An invoke with the model loaded first renders the panel;
                // a plain edit re-renders without a host call; a Back press
                // persists, then hands the message back to the host
                // Settings GUI (the re-render is the no-channel-id
                // fallback).
                let host_call = match (op.as_str(), cmd.as_deref()) {
                    ("view.interact", Some(COMMAND_NAME))
                    | ("view.interact", Some(FEED_SETTINGS_COMMAND_NAME)) => {
                        if let Err(error) = command::verify_settings_interaction(
                            args.as_ref().unwrap_or(&Value::Null),
                        ) {
                            if !reply_err(&mut out, id, "CommandError", error.to_string()) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        }
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
                        let mut session = session;
                        let effects = update(msg, &mut session.model);
                        if effects.is_empty() {
                            // A plain edit re-renders immediately.
                            if !reply_envelope(&mut out, id, &session) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        }
                        let snapshot = match effects.into_iter().next() {
                            Some(Effect::Persist(snapshot)) => snapshot,
                            None => unreachable!("Back and About always persist"),
                        };
                        if let Err(error) = service
                            .update_feed_settings(session.guild_id, snapshot)
                            .await
                        {
                            eprintln!("failed to persist feed settings: {error}");
                        }
                        let exit = match custom_id {
                            Some(CUSTOM_ID_ABOUT) => about_exit(args.as_ref(), session.guild_id),
                            _ => back_exit(args.as_ref(), session.guild_id),
                        };
                        let Some(exit) = exit else {
                            if !reply_envelope(&mut out, id, &session) {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        };
                        Some(HostCall::open_settings(
                            id,
                            session,
                            exit.message_id,
                            exit.args,
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
                if !issue_call(&output, &mut pending, &next_call_id, call) {
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
                let snapshot = match effects.into_iter().next() {
                    Some(Effect::Persist(snapshot)) => snapshot,
                    None => unreachable!("expiry always persists"),
                };
                if let Err(error) = service
                    .update_feed_settings(session.guild_id, snapshot)
                    .await
                {
                    eprintln!("failed to persist feed settings on expiry: {error}");
                }
            }
            Msg::Ping => {
                if write_msg(&mut out, &Msg::Pong).is_err() {
                    return ExitCode::FAILURE;
                }
            }
            Msg::Pong => {}
            Msg::Progress { .. } => {}
            // The host answers our hello with its own; tolerate it silently.
            Msg::Hello { .. } => {}
            Msg::Resp {
                id,
                ok,
                data,
                error,
            } => {
                let Some(pending_kind) = pending.remove(&id) else {
                    host_client.send_response(HostResponse {
                        id,
                        ok,
                        data,
                        error,
                    });
                    continue;
                };
                match pending_kind {
                    Pending::OpenSettings {
                        invoke_id,
                        session,
                        message_id,
                    } => {
                        if !ok {
                            eprintln!("host.open_view(settings) failed: {error:?}");
                        }
                        if ok && message_id.is_some() {
                            if !reply_err(&mut out, invoke_id, VIEW_MOVED_KIND, "settings opened") {
                                return ExitCode::FAILURE;
                            }
                            continue;
                        }
                        if !reply_envelope(&mut out, invoke_id, &session) {
                            return ExitCode::FAILURE;
                        }
                    }
                }
            }
        }
    }
    let _ = publisher.stop();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use pwr_plugin_support::open_settings_args;
    use pwr_poise_components::IS_COMPONENTS_V2;

    use super::*;

    fn model() -> Model {
        Model::new(FeedsSettings {
            enabled: Some(true),
            channel_id: Some("123456789".into()),
            subscribe_role_id: Some("987654321".into()),
            unsubscribe_role_id: Some("987654322".into()),
        })
    }

    #[test]
    fn toggle_flips_enabled() {
        let mut m = model();
        let effects = update(PanelMsg::ToggleEnabled, &mut m);
        assert!(effects.is_empty());
        assert!(!m.is_enabled());
    }

    #[test]
    fn toggle_defaults_to_enabled() {
        let mut m = Model::new(FeedsSettings::default());
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
    fn back_about_and_expiry_persist_the_snapshot_exactly_once() {
        for msg in [PanelMsg::Back, PanelMsg::About, PanelMsg::Expired] {
            let mut m = model();
            let effects = update(msg.clone(), &mut m);
            assert_eq!(effects.len(), 1, "{msg:?}");
            match &effects[0] {
                Effect::Persist(s) => assert_eq!(s.channel_id.as_deref(), Some("123456789")),
            }
        }
    }

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
            Some(SessionState::new(42, FeedsSettings::default())),
            "a string guild id parses, and sections default"
        );
    }

    #[test]
    fn panel_is_components_v2_without_legacy_content() {
        let data = view_data(&model());
        assert_eq!(data["flags"], json!(IS_COMPONENTS_V2));
        assert!(data.get("content").is_none(), "v2 carries no top content");
    }

    #[test]
    fn panel_has_the_expected_layout() {
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
            json!(concat!(
                "-# **Settings > Feeds**\n## Feed Subscription Settings\n\n",
                "> 🛈  Feed notifications are currently **active**. ",
                "Notifications will be sent to <#123456789>"
            ))
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
        let settings = FeedsSettings {
            enabled: Some(false),
            ..Default::default()
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

    #[test]
    fn session_payload_carries_the_guild_and_feed_settings() {
        let settings = model().settings;
        let payload = SessionState::new(7, settings.clone()).to_value();
        assert_eq!(payload["guild_id"], json!(7));
        assert_eq!(
            serde_json::from_value::<FeedsSettings>(payload["settings"].clone()).unwrap(),
            settings
        );
    }

    #[test]
    fn session_state_rejects_a_non_feed_settings_payload() {
        let payload = json!({
            "guild_id": 7,
            "settings": { "feeds": { "enabled": true } },
        });

        assert_eq!(SessionState::from_value(Some(&payload)), None);
    }

    #[test]
    fn open_settings_call_keeps_the_session_and_host_args() {
        let session = SessionState::new(1, FeedsSettings::default());
        let expected_session = session.clone();
        let call = HostCall::open_settings(9, session, Some(77), json!({ "plugin": "settings" }));

        let Pending::OpenSettings {
            invoke_id,
            session,
            message_id,
        } = call.pending;
        assert_eq!(invoke_id, 9);
        assert_eq!(session, expected_session);
        assert_eq!(message_id, Some(77));
        assert_eq!(call.args, json!({ "plugin": "settings" }));
    }

    #[test]
    fn open_settings_args_target_the_host_reserved_settings_and_edit_in_place() {
        assert_eq!(
            open_settings_args(5, 42, Some(777)),
            json!({
                "channel_id": 5,
                "plugin": "settings",
                "args": { "guild_id": 42 },
                "message_id": 777,
            })
        );
        assert_eq!(
            open_settings_args(5, 42, None)["message_id"],
            serde_json::Value::Null
        );
    }
}
