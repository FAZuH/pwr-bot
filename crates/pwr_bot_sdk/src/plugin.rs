use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

use crate::host::PluginHost;

/// A typed database parameter for parameterized SQL queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DbValue {
    Null,
    Bool(bool),
    I32(i32),
    I64(i64),
    F64(f64),
    Text(String),
}

impl From<()> for DbValue {
    fn from(_: ()) -> Self {
        DbValue::Null
    }
}

impl From<bool> for DbValue {
    fn from(v: bool) -> Self {
        DbValue::Bool(v)
    }
}

impl From<i32> for DbValue {
    fn from(v: i32) -> Self {
        DbValue::I32(v)
    }
}

impl From<i64> for DbValue {
    fn from(v: i64) -> Self {
        DbValue::I64(v)
    }
}

impl From<f64> for DbValue {
    fn from(v: f64) -> Self {
        DbValue::F64(v)
    }
}

impl From<String> for DbValue {
    fn from(v: String) -> Self {
        DbValue::Text(v)
    }
}

impl From<&str> for DbValue {
    fn from(v: &str) -> Self {
        DbValue::Text(v.to_string())
    }
}

/// Describes a single command argument for slash command registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgSpec {
    pub name: String,
    pub description: String,
    pub kind: String,
}

/// Describes a slash command provided by the plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSpec {
    pub name: String,
    pub description: String,
    pub args: Vec<ArgSpec>,
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
}

impl TestStepSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
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
    pub fn new(
        name: impl Into<String>,
        interval_secs: u64,
        command: impl Into<String>,
    ) -> Self {
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
/// The host calls [`commands`] at load time to learn what slash commands
/// the plugin provides, then calls [`invoke`] when a user triggers one.
#[async_trait]
pub trait BotPlugin: Send + Sync {
    fn name(&self) -> &'static str;

    fn description(&self) -> &'static str;

    fn version(&self) -> &'static str;

    fn commands(&self) -> Vec<CommandSpec>;

    async fn invoke(
        &self,
        command: &str,
        args: serde_json::Value,
        host: &PluginHost,
    ) -> Result<ResponsePayload, String>;

    /// Called once after the plugin is loaded and registered.
    async fn init(&self, _host: &PluginHost) -> Result<(), String> {
        Ok(())
    }

    /// Called before the bot shuts down.
    async fn shutdown(&self) -> Result<(), String> {
        Ok(())
    }

    /// Called when an event this plugin subscribed to fires.
    async fn on_event(
        &self,
        _event_name: &str,
        _payload: serde_json::Value,
        _host: &PluginHost,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Declares which events this plugin handles.
    fn event_handlers(&self) -> Vec<EventHandlerSpec> {
        vec![]
    }

    /// Declares settings panels this plugin contributes.
    fn settings_panels(&self) -> Vec<SettingsPanelSpec> {
        vec![]
    }

    /// Declares GUI test steps this plugin provides.
    fn test_steps(&self) -> Vec<TestStepSpec> {
        vec![]
    }

    /// Declares background tasks this plugin wants the host to spawn.
    fn tasks(&self) -> Vec<TaskSpec> {
        vec![]
    }
}
