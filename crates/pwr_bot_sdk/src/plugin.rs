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
}
