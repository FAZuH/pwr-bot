//! Registry mapping plugin command names to their loaded plugins.
//!
//! Also constructs poise `Command` objects from plugin `CommandDescriptor`s
//! so the framework can route slash commands to plugins.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use log::info;
use poise::Command;
use pwr_bot_sdk::BotPlugin;
use pwr_bot_sdk::CommandSpec;
use pwr_bot_sdk::EventHandlerSpec;
use pwr_bot_sdk::SettingsPanelSpec;
use pwr_bot_sdk::TaskSpec;
use pwr_bot_sdk::TestStepSpec;
use tokio::sync::RwLock;

use crate::bot::Data;
use crate::bot::command::Error;
use crate::bot::plugin::loader::LoadedPlugin;

/// Thread-safe registry of loaded plugins.
pub struct PluginRegistry {
    ffi_plugins: RwLock<Vec<Arc<LoadedPlugin>>>,
    builtin_plugins: RwLock<Vec<Arc<dyn BotPlugin + 'static>>>,
    /// Command name → (is_builtin, index)
    command_map: RwLock<HashMap<String, (bool, usize)>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            ffi_plugins: RwLock::new(Vec::new()),
            builtin_plugins: RwLock::new(Vec::new()),
            command_map: RwLock::new(HashMap::new()),
        }
    }

    /// Register a loaded `.so` plugin and return its poise `Command` list.
    pub async fn register(&self, plugin: LoadedPlugin) -> Vec<Command<Data, Error>> {
        let specs = plugin.metadata.commands.clone();

        let plugin = Arc::new(plugin);
        let idx = {
            let mut plugins = self.ffi_plugins.write().await;
            let idx = plugins.len();
            plugins.push(plugin.clone());
            idx
        };

        let mut cmds = Vec::new();
        for spec in &specs {
            let cmd_name = spec.name.clone();
            self.command_map
                .write()
                .await
                .insert(cmd_name.clone(), (false, idx));
            cmds.push(build_plugin_command(spec));
            info!(
                "Registered plugin command: /{cmd_name} (from {})",
                plugin.name
            );
        }

        cmds
    }

    /// Register a compiled-in `BotPlugin` and return its poise `Command` list.
    pub async fn register_builtin(
        &self,
        plugin: Box<dyn BotPlugin + 'static>,
    ) -> Vec<Command<Data, Error>> {
        let name = plugin.name().to_string();
        let specs = plugin.commands();

        let plugin = Arc::from(plugin);
        let idx = {
            let mut builtin = self.builtin_plugins.write().await;
            let idx = builtin.len();
            builtin.push(plugin);
            idx
        };

        let mut cmds = Vec::new();
        for spec in &specs {
            let cmd_name = spec.name.clone();
            self.command_map
                .write()
                .await
                .insert(cmd_name.clone(), (true, idx));
            cmds.push(build_plugin_command(spec));
            info!("Registered builtin plugin command: /{cmd_name} (from {name})");
        }

        cmds
    }

    /// Look up a plugin by command name.
    ///
    /// Returns `(is_builtin, index, Arc<...>)`.
    pub async fn lookup(&self, command_name: &str) -> Option<(bool, usize, PluginRef)> {
        let (is_builtin, idx) = self.command_map.read().await.get(command_name).copied()?;

        if is_builtin {
            let plugin = self.builtin_plugins.read().await[idx].clone();
            Some((true, idx, PluginRef::Builtin(plugin)))
        } else {
            let plugin = self.ffi_plugins.read().await[idx].clone();
            Some((false, idx, PluginRef::Ffi(plugin)))
        }
    }

    // ---- Plugin discovery methods ----

    /// Returns all loaded settings panels across all plugins.
    pub async fn all_settings_panels(&self) -> Vec<(String, String, SettingsPanelSpec)> {
        let mut panels = Vec::new();
        for plugin in self.builtin_plugins.read().await.iter() {
            let name = plugin.name().to_string();
            for panel in plugin.settings_panels() {
                panels.push((name.clone(), plugin.name().to_string(), panel));
            }
        }
        // Also check FFI plugins (handled via LoadedPlugin.metadata)
        for plugin in self.ffi_plugins.read().await.iter() {
            let name = plugin.name.clone();
            for panel in &plugin.metadata.settings_panels {
                panels.push((name.clone(), name.clone(), panel.clone()));
            }
        }
        panels
    }

    /// Returns all registered event handlers across all plugins.
    pub async fn all_event_handlers(&self) -> Vec<(Arc<dyn BotPlugin + 'static>, EventHandlerSpec)> {
        let mut handlers = Vec::new();
        for plugin in self.builtin_plugins.read().await.iter() {
            for spec in plugin.event_handlers() {
                handlers.push((plugin.clone(), spec));
            }
        }
        handlers
    }

    /// Returns all declared test steps across all plugins.
    pub async fn all_test_steps(&self) -> Vec<(String, String, TestStepSpec)> {
        let mut steps = Vec::new();
        for plugin in self.builtin_plugins.read().await.iter() {
            let name = plugin.name().to_string();
            for spec in plugin.test_steps() {
                steps.push((name.clone(), spec.name.clone(), spec));
            }
        }
        for plugin in self.ffi_plugins.read().await.iter() {
            for spec in &plugin.metadata.test_steps {
                steps.push((plugin.name.clone(), spec.name.clone(), spec.clone()));
            }
        }
        steps
    }

    /// Returns all declared tasks across all plugins.
    pub async fn all_tasks(&self) -> Vec<(String, TaskSpec)> {
        let mut tasks = Vec::new();
        for plugin in self.builtin_plugins.read().await.iter() {
            let name = plugin.name().to_string();
            for spec in plugin.tasks() {
                tasks.push((name.clone(), spec));
            }
        }
        for plugin in self.ffi_plugins.read().await.iter() {
            for spec in &plugin.metadata.tasks {
                tasks.push((plugin.name.clone(), spec.clone()));
            }
        }
        tasks
    }

    /// Returns all builtin plugins.
    pub fn builtin_plugins(&self) -> Vec<Arc<dyn BotPlugin + 'static>> {
        let guard = self.builtin_plugins.blocking_read();
        guard.clone()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Enum for a plugin reference (either FFI or builtin).
pub enum PluginRef {
    Builtin(Arc<dyn BotPlugin + 'static>),
    Ffi(Arc<LoadedPlugin>),
}

/// Build a poise `Command` from a plugin `CommandSpec`.
fn build_plugin_command(spec: &CommandSpec) -> Command<Data, Error> {
    Command::<Data, Error> {
        name: Cow::Owned(spec.name.clone()),
        description: Some(Cow::Owned(spec.description.clone())),
        ..Default::default()
    }
}
