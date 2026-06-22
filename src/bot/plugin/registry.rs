//! Registry mapping plugin command names to their loaded plugins.
//!
//! Each plugin provides [`CommandDefinition`]s whose `data` field is a
//! serialized `serenity::CreateCommand` JSON.  The host extracts the
//! top‑level name and sub‑command names for poise routing, stores the
//! raw JSON for Discord registration, and attaches the static
//! [`registry_command_handler`] as the slash action.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use poise::Command;
use pwr_bot_sdk::CommandDefinition;
use pwr_bot_sdk::EventHandlerSpec;
use pwr_bot_sdk::SettingsPanelSpec;
use pwr_bot_sdk::TaskSpec;
use pwr_bot_sdk::TestStepSpec;
use tokio::sync::RwLock;
use tracing::info;

use crate::bot::Data;
use crate::bot::command::Error;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_plugin_command;
use crate::bot::plugin::loader::LoadedPlugin;

/// Thread-safe registry of loaded plugins.
pub struct PluginRegistry {
    plugins: RwLock<Vec<Arc<LoadedPlugin>>>,
    /// Full command name (e.g. `"feed"`, `"feed subscribe"`) → plugin index
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
        let cmd_defs = plugin.metadata.commands.clone();

        let plugin = Arc::new(plugin);
        let idx = {
            let mut plugins = self.plugins.write().await;
            let idx = plugins.len();
            plugins.push(plugin.clone());
            idx
        };

        let mut cmds = Vec::new();
        let mut map = self.command_map.write().await;
        for def in &cmd_defs {
            let top_name = def.name.clone();
            let sub_names = extract_subcommand_names(&def.data);

            // Register all full command names in the map
            map.insert(top_name.clone(), idx);
            for sub in &sub_names {
                map.insert(format!("{top_name} {sub}"), idx);
            }

            cmds.push(build_routing_command(def));

            let full_names = if sub_names.is_empty() {
                top_name.clone()
            } else {
                format!("{top_name} [{}]", sub_names.join(", "))
            };
            info!(
                command.name = %full_names,
                plugin.name = %plugin.name,
                "plugin command registered",
            );
        }

        cmds
    }

    /// Collect all plugin `CreateCommand` JSONs for Discord registration.
    pub async fn all_command_data(&self) -> Vec<serde_json::Value> {
        let mut data = Vec::new();
        for plugin in self.plugins.read().await.iter() {
            for def in &plugin.metadata.commands {
                data.push(def.data.clone());
            }
        }
        data
    }

    /// Returns plugin `CreateCommand` JSONs keyed by top-level command name.
    ///
    /// Used during registration to replace poise-generated `CreateCommand`s
    /// (which have empty parameters for plugin commands) with the plugin's
    /// full specification.
    pub async fn all_command_data_by_name(&self) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();
        for plugin in self.plugins.read().await.iter() {
            for def in &plugin.metadata.commands {
                map.insert(def.name.clone(), def.data.clone());
            }
        }
        map
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
    pub async fn all_commands(&self) -> Vec<Command<Data, Error>> {
        let mut cmds = Vec::new();
        for plugin in self.plugins.read().await.iter() {
            for def in &plugin.metadata.commands {
                cmds.push(build_routing_command(def));
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

/// Build a minimal poise `Command` for routing from a serialised
/// `CreateCommand`.  The actual parameter spec lives in the plugin JSON
/// and is sent directly to Discord during registration.
fn build_routing_command(def: &CommandDefinition) -> Command<Data, Error> {
    let description = def
        .data
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let sub_names = extract_subcommand_names(&def.data);

    Command::<Data, Error> {
        name: Cow::Owned(def.name.clone()),
        description: Some(Cow::Owned(description)),
        slash_action: Some(|ctx| Box::pin(registry_command_handler(ctx))),
        subcommands: sub_names
            .into_iter()
            .map(|name| Command::<Data, Error> {
                name: Cow::Owned(name),
                slash_action: Some(|ctx| Box::pin(registry_command_handler(ctx))),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

/// Extract subcommand names from a serialised `CreateCommand` JSON.
///
/// Discord `CommandOptionType::SubCommand` has value `1`.
fn extract_subcommand_names(data: &serde_json::Value) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(options) = data.get("options").and_then(|v| v.as_array()) {
        for opt in options {
            if opt.get("type").and_then(|v| v.as_u64()) == Some(1)
                && let Some(name) = opt.get("name").and_then(|v| v.as_str())
            {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// Static handler for all registry-resolved plugin slash commands.
///
/// Resolves the plugin from the command name at runtime via the registry
/// stored in [`Data`], then dispatches through [`dispatch_plugin_command`].
async fn registry_command_handler<'a>(
    ctx: poise::ApplicationContext<'a, Data, Error>,
) -> Result<(), poise::FrameworkError<'a, Data, Error>> {
    let args_json = serde_json::to_value(&ctx.interaction.data).unwrap_or(serde_json::Value::Null);

    let cmd_name = {
        let mut parts = vec![ctx.interaction.data.name.to_string()];
        if let Some(first) = ctx.interaction.data.options.first() {
            use poise::serenity_prelude::CommandOptionType;
            if first.kind() == CommandOptionType::SubCommand {
                parts.push(first.name.to_string());
            }
        }
        parts.join(" ")
    };

    let poise_ctx: poise::Context<'a, Data, Error> = ctx.into();

    let guild_id = poise_ctx.guild_id().map(|g| g.get());
    let author_id = poise_ctx.author().id.get();

    let span = tracing::info_span!("registry_command_handler", command.name = %cmd_name, guild.id = guild_id);
    let _guard = span.enter();

    tracing::info!(user.id = author_id, "command invoked",);

    let registry = &poise_ctx.data().plugin_registry;
    let host_ctx = Arc::new(PoiseHostCtx::new(poise_ctx));

    if let Err(e) = dispatch_plugin_command(registry, &host_ctx, &cmd_name, args_json).await {
        tracing::error!(error = %e, "plugin command failed");
    }
    Ok(())
}
