//! Registry mapping plugin command names to their loaded plugins.
//!
//! Also constructs poise `Command` objects from plugin `CommandDescriptor`s
//! so the framework can route slash commands to plugins.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::Arc;

use log::info;
use poise::Command;
use pwr_bot_sdk::CommandSpec;
use tokio::sync::RwLock;

use crate::bot::Data;
use crate::bot::command::Error;
use crate::bot::plugin::loader::LoadedPlugin;

/// Thread-safe registry of loaded plugins.
pub struct PluginRegistry {
    plugins: RwLock<Vec<Arc<LoadedPlugin>>>,
    /// Command name → plugin index in `plugins`
    command_map: RwLock<HashMap<String, usize>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: RwLock::new(Vec::new()),
            command_map: RwLock::new(HashMap::new()),
        }
    }

    /// Register a loaded plugin and return its poise `Command` list.
    pub async fn register(&self, plugin: LoadedPlugin) -> Vec<Command<Data, Error>> {
        let vtable = plugin.vtable;

        // Get command descriptors from the plugin
        let cmd_list = unsafe { (vtable.commands)() };

        let commands_json = if cmd_list.json.is_null() {
            "[]"
        } else {
            unsafe { CStr::from_ptr(cmd_list.json).to_str().unwrap_or("[]") }
        };

        let specs: Vec<pwr_bot_sdk::CommandSpec> =
            serde_json::from_str(commands_json).unwrap_or_default();

        let plugin = Arc::new(plugin);
        let idx = {
            let mut plugins = self.plugins.write().await;
            let idx = plugins.len();
            plugins.push(plugin.clone());
            idx
        };

        let mut cmds = Vec::new();
        for spec in &specs {
            let cmd_name = spec.name.clone();
            self.command_map.write().await.insert(cmd_name.clone(), idx);

            let command = build_plugin_command(plugin.clone(), spec);
            cmds.push(command);

            info!(
                "Registered plugin command: /{} (from {})",
                cmd_name, plugin.name
            );
        }

        cmds
    }
}

/// Build a poise `Command` from a plugin `CommandSpec`.
fn build_plugin_command(_plugin: Arc<LoadedPlugin>, spec: &CommandSpec) -> Command<Data, Error> {
    let cmd_name = spec.name.clone();

    Command::<Data, Error> {
        name: Cow::Owned(cmd_name.clone()),
        description: Some(Cow::Owned(spec.description.clone())),
        ..Default::default()
    }
}
