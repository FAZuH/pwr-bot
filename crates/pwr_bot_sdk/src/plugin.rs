use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

use crate::host::PluginHost;

/// Describes a single command argument for slash command registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgSpec {
    pub name: String,
    pub description: String,
    pub kind: String,
}

/// Describes a slash command provided by the plugin.
///
/// A command with a space-separated name (e.g. `"feed list"`) is registered
/// as a subcommand of the parent (`"feed"` → `/feed list`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSpec {
    pub name: String,
    pub description: String,
    pub args: Vec<ArgSpec>,
}

impl CommandSpec {
    /// Creates a new command spec with the given name and description.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            args: vec![],
        }
    }
}

/// Payload returned by a plugin after handling a command invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponsePayload {
    pub content: Option<String>,
    pub ephemeral: bool,
    pub components_json: Option<serde_json::Value>,
    pub embed_json: Option<serde_json::Value>,
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
    pub commands: Vec<CommandSpec>,
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
/// use pwr_bot_sdk::{BotPlugin, CommandSpec, ResponsePayload, PluginHost, export_plugin};
///
/// struct MyPlugin;
///
/// #[async_trait::async_trait]
/// impl BotPlugin for MyPlugin {
///     fn name(&self) -> &'static str { "my_plugin" }
///     fn description(&self) -> &'static str { "My first plugin" }
///     fn version(&self) -> &'static str { "0.1.0" }
///
///     fn commands(&self) -> Vec<CommandSpec> {
///         vec![CommandSpec {
///             name: "hello".into(),
///             description: "Says hello".into(),
///             args: vec![],
///         }]
///     }
///
///     async fn invoke(&self, command: &str, _args: serde_json::Value, host: &PluginHost) -> Result<ResponsePayload, String> {
///         host.send_reply("Hello from my plugin!").await;
///         Ok(ResponsePayload { content: None, ephemeral: false, components_json: None, embed_json: None })
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
    fn commands(&self) -> Vec<CommandSpec>;

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
