//! Registry mapping plugin command names to their loaded plugins.
//!
//! Also constructs poise `Command` objects from plugin `CommandDescriptor`s
//! so the framework can route slash commands to plugins.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use tracing::info;
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
use tracing::instrument;

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
                command.name = %cmd_name,
                plugin.name = %plugin.name,
                "plugin command registered",
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
    /// Commands with space-separated names (e.g. `"feed list"`) are nested as
    /// subcommands of their parent (`"feed"`), producing a proper Discord
    /// command tree that shows subcommands in the autocomplete UI.
    pub async fn all_commands(&self) -> Vec<Command<Data, Error>> {
        // Collect all specs, grouping by parent name (the token before the first space).
        struct Group {
            root: Option<CommandSpec>,
            sub: Vec<CommandSpec>,
        }
        let mut groups: HashMap<String, Group> = HashMap::new();

        for plugin in self.plugins.read().await.iter() {
            for spec in &plugin.metadata.commands {
                if let Some((parent, _sub)) = spec.name.split_once(' ') {
                    let g = groups.entry(parent.to_string()).or_insert(Group {
                        root: None,
                        sub: Vec::new(),
                    });
                    g.sub.push(spec.clone());
                } else {
                    let g = groups.entry(spec.name.clone()).or_insert(Group {
                        root: None,
                        sub: Vec::new(),
                    });
                    g.root = Some(spec.clone());
                }
            }
        }

        let mut cmds = Vec::new();
        for (_name, group) in groups {
            let root = group.root.unwrap_or_else(|| CommandSpec {
                name: _name.clone(),
                description: String::new(),
                args: vec![],
            });

            let mut cmd = build_registry_command(&root);
            if !group.sub.is_empty() {
                cmd.subcommands = group
                    .sub
                    .iter()
                    .map(|spec| {
                        let mut sub_cmd = build_registry_command(spec);
                        // Strip parent prefix for Discord-compatible subcommand name
                        if let Some((_, sub_name)) = spec.name.split_once(' ') {
                            sub_cmd.name = Cow::Owned(sub_name.to_string());
                        }
                        sub_cmd
                    })
                    .collect();
            }
            cmds.push(cmd);
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
#[instrument(skip_all, fields(command.name = tracing::field::Empty, guild.id = tracing::field::Empty))]
async fn registry_command_handler<'a>(
    ctx: poise::ApplicationContext<'a, Data, Error>,
) -> Result<(), poise::FrameworkError<'a, Data, Error>> {
    let poise_ctx: poise::Context<'a, Data, Error> = ctx.into();

    let cmd_name = poise_ctx.invoked_command_name();
    let guild_id = poise_ctx.guild_id().map(|g| g.get());
    let author_id = poise_ctx.author().id.get();
    tracing::Span::current().record("command.name", &cmd_name);
    if let Some(gid) = guild_id {
        tracing::Span::current().record("guild.id", gid);
    }

    tracing::info!(
        command.name = %cmd_name,
        guild.id = guild_id,
        user.id = author_id,
        "command invoked",
    );
    tracing::debug!(
        command.name = %cmd_name,
        guild.id = guild_id,
        user.id = author_id,
        channel.id = poise_ctx.channel_id().get(),
        "command dispatch started",
    );

    let registry = &poise_ctx.data().plugin_registry;
    let host_ctx = Arc::new(PoiseHostCtx::new(poise_ctx));

    if let Err(e) =
        dispatch_plugin_command(registry, &host_ctx, cmd_name, serde_json::Value::Null).await
    {
        tracing::error!(command.name = %cmd_name, error = %e, "plugin command failed");
    } else {
        tracing::debug!(command.name = %cmd_name, "command dispatch completed");
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
