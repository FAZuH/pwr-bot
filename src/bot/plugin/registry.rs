//! Registry mapping plugin command names to their loaded plugins.
//!
//! Also constructs poise `Command` objects from plugin `CommandDescriptor`s
//! so the framework can route slash commands to plugins.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use log::info;
use poise::Command;
use pwr_bot_sdk::CommandSpec;
use pwr_bot_sdk::EventHandlerSpec;
use pwr_bot_sdk::SettingsPanelSpec;
use pwr_bot_sdk::TaskSpec;
use pwr_bot_sdk::TestStepSpec;
use tokio::sync::RwLock;

use crate::bot::Data;
use crate::bot::command::Error;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_plugin_command;
use crate::bot::plugin::loader::LoadedPlugin;

/// Thread-safe registry of loaded plugins.
pub struct PluginRegistry {
    plugins: RwLock<Vec<Arc<LoadedPlugin>>>,
    /// Command name → plugin index
    command_map: RwLock<HashMap<String, usize>>,
}

impl PluginRegistry {
    /// Creates an empty plugin registry.
    pub fn new() -> Self {
        Self {
            plugins: RwLock::new(Vec::new()),
            command_map: RwLock::new(HashMap::new()),
        }
    }

    /// Register a loaded `.so` plugin and return its poise `Command` list.
    pub async fn register(&self, plugin: LoadedPlugin) -> Vec<Command<Data, Error>> {
        let specs = plugin.metadata.commands.clone();

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
            cmds.push(build_plugin_command(spec));
            info!(
                "Registered plugin command: /{cmd_name} (from {})",
                plugin.name
            );
        }

        cmds
    }

    /// Look up a plugin by command name.
    pub async fn lookup(&self, command_name: &str) -> Option<(usize, Arc<LoadedPlugin>)> {
        let idx = self.command_map.read().await.get(command_name).copied()?;
        let plugin = self.plugins.read().await[idx].clone();
        Some((idx, plugin))
    }

    // ---- Plugin discovery methods ----

    /// Returns all loaded settings panels across all plugins.
    pub async fn all_settings_panels(&self) -> Vec<(String, String, SettingsPanelSpec)> {
        let mut panels = Vec::new();
        for plugin in self.plugins.read().await.iter() {
            let name = plugin.name.clone();
            for panel in &plugin.metadata.settings_panels {
                panels.push((name.clone(), name.clone(), panel.clone()));
            }
        }
        panels
    }

    /// Returns all registered event handlers across all plugins.
    pub async fn all_event_handlers(&self) -> Vec<(Arc<LoadedPlugin>, EventHandlerSpec)> {
        let mut handlers = Vec::new();
        for plugin in self.plugins.read().await.iter() {
            for spec in &plugin.metadata.event_handlers {
                handlers.push((plugin.clone(), spec.clone()));
            }
        }
        handlers
    }

    /// Returns all declared test steps across all plugins.
    pub async fn all_test_steps(&self) -> Vec<(String, String, TestStepSpec)> {
        let mut steps = Vec::new();
        for plugin in self.plugins.read().await.iter() {
            for spec in &plugin.metadata.test_steps {
                steps.push((plugin.name.clone(), spec.name.clone(), spec.clone()));
            }
        }
        steps
    }

    /// Returns all declared tasks across all plugins.
    pub async fn all_tasks(&self) -> Vec<(String, TaskSpec)> {
        let mut tasks = Vec::new();
        for plugin in self.plugins.read().await.iter() {
            for spec in &plugin.metadata.tasks {
                tasks.push((plugin.name.clone(), spec.clone()));
            }
        }
        tasks
    }

    /// Returns all loaded plugins.
    pub async fn all_ffi_plugins(&self) -> Vec<Arc<LoadedPlugin>> {
        self.plugins.read().await.clone()
    }

    /// Returns Poise `Command`s for all registered plugins.
    ///
    /// Each command resolves its plugin at runtime via [`dispatch_plugin_command`],
    /// so no core Poise wrappers are needed. Plugin authors add commands simply
    /// by implementing [`BotPlugin::commands`](pwr_bot_sdk::BotPlugin::commands) — no core changes required.
    pub async fn all_commands(&self) -> Vec<Command<Data, Error>> {
        let mut cmds = Vec::new();

        for plugin in self.plugins.read().await.iter() {
            for spec in &plugin.metadata.commands {
                cmds.push(build_registry_command(spec));
            }
        }

        cmds
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a poise `Command` from a plugin `CommandSpec` (metadata-only, no handler).
fn build_plugin_command(spec: &CommandSpec) -> Command<Data, Error> {
    Command::<Data, Error> {
        name: Cow::Owned(spec.name.clone()),
        description: Some(Cow::Owned(spec.description.clone())),
        ..Default::default()
    }
}

/// Static handler for all registry-resolved plugin slash commands.
///
/// Resolves the plugin from the command name at runtime via the registry
/// stored in [`Data`], then dispatches through [`dispatch_plugin_command`].
/// Errors are logged rather than propagated (Poise's `FrameworkError::Command`
/// is `#[non_exhaustive]` and cannot be constructed externally).
async fn registry_command_handler<'a>(
    ctx: poise::ApplicationContext<'a, Data, Error>,
) -> Result<(), poise::FrameworkError<'a, Data, Error>> {
    let poise_ctx: poise::Context<'a, Data, Error> = ctx.into();
    let cmd_name = poise_ctx.invoked_command_name();
    let registry = &poise_ctx.data().plugin_registry;
    let host_ctx = Arc::new(PoiseHostCtx::new(poise_ctx));

    if let Err(e) =
        dispatch_plugin_command(registry, &host_ctx, cmd_name, serde_json::Value::Null).await
    {
        log::error!("Plugin command '{cmd_name}' failed: {e}");
    }
    Ok(())
}

/// Build a poise `Command` from a plugin `CommandSpec` for registry-based dispatch.
///
/// The command uses a static handler that resolves the plugin from the
/// registry at dispatch time.
fn build_registry_command(spec: &CommandSpec) -> Command<Data, Error> {
    Command::<Data, Error> {
        name: Cow::Owned(spec.name.clone()),
        description: Some(Cow::Owned(spec.description.clone())),
        slash_action: Some(|ctx| Box::pin(registry_command_handler(ctx))),
        ..Default::default()
    }
}
