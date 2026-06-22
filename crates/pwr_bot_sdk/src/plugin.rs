use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

use crate::host::PluginHost;

/// A command definition produced by a plugin.
///
/// The `data` field contains a serialized `serenity::CreateCommand` — the
/// exact JSON that will be sent to Discord's command registration API. The
/// host uses `name` for slash-command routing and `data` for registration.
///
/// Plugins build this by constructing a `serenity::builder::CreateCommand`
/// with the full builder API (choices, autocomplete, channel types, etc.)
/// and serializing it:
///
/// ```ignore
/// use serenity::builder::CreateCommand;
/// use serenity::model::application::CommandOptionType;
///
/// let cmd = CreateCommand::new("feed")
///     .description("Manage feed subscriptions")
///     .add_option(
///         CreateCommandOption::new("subscribe", "Subscribe to feeds")
///             .kind(CommandOptionType::SubCommand)
///             .add_sub_option(
///                 CreateCommandOption::new("links", "Feed URLs")
///                     .kind(CommandOptionType::String)
///                     .required(true)
///             )
///     );
///
/// let data = serde_json::to_value(&cmd).expect("CreateCommand serialization");
/// let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
/// CommandDefinition { name, data }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDefinition {
    /// Top-level command name (e.g. `"feed"`), used for routing.
    pub name: String,
    /// Serialized `serenity::CreateCommand` JSON, sent to Discord as-is.
    pub data: serde_json::Value,
}

/// Payload returned by a plugin after handling a command invocation.
///
/// Wraps raw JSON that the host forwards to Discord's API. The plugin can
/// construct it from any poise/serenity type that implements [`Serialize`]
/// (e.g. `CreateReply`, `CreateEmbed`, `CreateComponent`, `CreateButton`,
/// `CreateActionRow`, `CreateSelectMenu`, `EditMessage`, etc.):
///
/// ```ignore
/// use pwr_bot_sdk::ResponsePayload;
///
/// let reply = serde_json::json!({"content": "Hello!", "flags": 64});
/// let payload = ResponsePayload::from_serializable(&reply).unwrap();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsePayload {
    /// Raw JSON data portion of the Discord interaction response.
    ///
    /// For interaction replies this becomes `data` in `{"type": 4, "data": …}`.
    /// For channel messages / DMs / edits the value is sent as-is.
    pub data: serde_json::Value,
}

impl ResponsePayload {
    /// Creates a response from any `Serialize`-able value (poise reply builders,
    /// serenity embeds/components, or raw `serde_json::Value`).
    pub fn from_serializable(data: &impl Serialize) -> Result<Self, serde_json::Error> {
        Ok(Self {
            data: serde_json::to_value(data)?,
        })
    }

    /// Plain text response (non-ephemeral).
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            data: serde_json::json!({"content": content.into()}),
        }
    }

    /// Plain text response, ephemeral.
    pub fn text_ephemeral(content: impl Into<String>) -> Self {
        Self {
            data: serde_json::json!({"content": content.into(), "flags": 64}),
        }
    }

    /// Empty response — tells the host to skip sending any message.
    ///
    /// Use this when the plugin has already responded via [`send_reply`] or
    /// [`edit_reply`](crate::PluginHost::edit_reply) during the invoke call.
    pub fn none() -> Self {
        Self {
            data: serde_json::Value::Null,
        }
    }
}

/// Specifies an event the plugin wants to handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventHandlerSpec {
    pub event_name: String,
}

impl EventHandlerSpec {
    /// Creates a new event handler spec for the given event name.
    pub const fn new(event_name: String) -> Self {
        Self { event_name }
    }
}

/// Specifies a settings panel the plugin contributes to the settings UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsPanelSpec {
    pub id: String,
    pub label: String,
}

impl SettingsPanelSpec {
    /// Creates a new settings panel spec with the given `id` and display `label`.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// Specifies a GUI test step provided by the plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestStepSpec {
    pub name: String,
    pub description: String,
    pub command: String,
    pub args: serde_json::Value,
}

impl TestStepSpec {
    /// Creates a new test step with the given `name`, `description`, `command`, and `args`.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        command: impl Into<String>,
        args: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            command: command.into(),
            args,
        }
    }
}

/// Specifies a background task the plugin wants the host to spawn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub name: String,
    pub interval_secs: u64,
    pub command: String,
}

impl TaskSpec {
    /// Creates a new task spec that calls `command` every `interval_secs` seconds.
    pub fn new(name: impl Into<String>, interval_secs: u64, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            interval_secs,
            command: command.into(),
        }
    }
}

/// Plugin metadata returned by the FFI `metadata()` function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub api_version: u32,
    pub name: String,
    pub description: String,
    pub version: String,
    pub commands: Vec<CommandDefinition>,
    pub event_handlers: Vec<EventHandlerSpec>,
    pub settings_panels: Vec<SettingsPanelSpec>,
    pub test_steps: Vec<TestStepSpec>,
    pub tasks: Vec<TaskSpec>,
}

/// Trait that plugin authors implement to define bot logic.
///
/// The host calls [`Self::commands`] at load time to learn what slash commands
/// the plugin provides, then calls [`Self::invoke`] when a user triggers one.
///
/// # Example
///
/// ```ignore
/// use pwr_bot_sdk::{BotPlugin, CommandDefinition, ResponsePayload, PluginHost, export_plugin};
///
/// struct MyPlugin;
///
/// #[async_trait::async_trait]
/// impl BotPlugin for MyPlugin {
///     fn name(&self) -> &'static str { "my_plugin" }
///     fn description(&self) -> &'static str { "My first plugin" }
///     fn version(&self) -> &'static str { "0.1.0" }
///
///     fn commands(&self) -> Vec<CommandDefinition> {
///         let cmd = serde_json::json!({
///             "name": "hello",
///             "description": "Says hello",
///             "options": []
///         });
///         vec![CommandDefinition {
///             name: "hello".into(),
///             data: cmd,
///         }]
///     }
///
///     async fn invoke(&self, command: &str, _args: serde_json::Value, host: &PluginHost) -> Result<ResponsePayload, String> {
///         host.send_reply("Hello from my plugin!").await;
///         Ok(ResponsePayload::text("done"))
///     }
/// }
///
/// export_plugin!(MyPlugin, MyPlugin);
/// ```
#[async_trait]
pub trait BotPlugin: Send + Sync {
    /// Returns the unique name of this plugin (e.g. `"voice_tracking"`).
    fn name(&self) -> &'static str;

    /// Returns a human-readable description of what this plugin does.
    fn description(&self) -> &'static str;

    /// Returns the semantic version of this plugin.
    fn version(&self) -> &'static str;

    /// Declares the slash commands this plugin provides.
    ///
    /// Called at load time by the host to register commands with Discord.
    fn commands(&self) -> Vec<CommandDefinition>;

    /// Handles a slash command invocation.
    ///
    /// Called when a user triggers a command declared by [`Self::commands`].
    /// Returns a [`ResponsePayload`] describing how the host should respond.
    async fn invoke(
        &self,
        command: &str,
        args: serde_json::Value,
        host: &PluginHost,
    ) -> Result<ResponsePayload, String>;

    /// Called once after the plugin is loaded and registered.
    ///
    /// Use this for one-time setup (connecting to databases, spawning internal
    /// tasks, etc.). If this returns an error the plugin will be unloaded.
    async fn init(&self, _host: &PluginHost) -> Result<(), String> {
        Ok(())
    }

    /// Called before the bot shuts down.
    ///
    /// Use this to flush state, close connections, or persist in-memory data.
    async fn shutdown(&self) -> Result<(), String> {
        Ok(())
    }

    /// Called when an event this plugin subscribed to fires.
    ///
    /// Subscribe to events by returning their names from [`Self::event_handlers`].
    /// The `payload` is the serialized event data as a JSON value.
    async fn on_event(
        &self,
        _event_name: &str,
        _payload: serde_json::Value,
        _host: &PluginHost,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Declares which events this plugin handles.
    ///
    /// Events are named strings (e.g. `"voice_state"`). The host subscribes
    /// the plugin to each event at load time.
    fn event_handlers(&self) -> Vec<EventHandlerSpec> {
        vec![]
    }

    /// Declares settings panels this plugin contributes to the settings UI.
    fn settings_panels(&self) -> Vec<SettingsPanelSpec> {
        vec![]
    }

    /// Declares GUI test steps this plugin provides for the test framework.
    fn test_steps(&self) -> Vec<TestStepSpec> {
        vec![]
    }

    /// Declares background tasks this plugin wants the host to spawn.
    ///
    /// Each task runs periodically at its configured interval. The task invokes
    /// the plugin's [`Self::invoke`] with the configured command.
    fn tasks(&self) -> Vec<TaskSpec> {
        vec![]
    }
}
