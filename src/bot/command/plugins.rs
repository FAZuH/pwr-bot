//! Admin plugin management commands: list, enable, disable, and swap plugins
//! from the external catalog.
//!
//! Subcommands are admin-gated (see
//! [`is_author_guild_admin`](crate::bot::checks::is_author_guild_admin)) and
//! guild-scoped: enablement state lives in the `guild_plugins` table, and
//! command registration targets the invoking guild. Pure state transitions
//! run through [`PluginsUpdate`](crate::update::PluginsUpdate); the returned
//! [`PluginsCmd`](crate::update::PluginsCmd) drives the Discord/DB side
//! effects.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pwr_plugin_protocol::Manifest;

use crate::bot::command::prelude::*;
use crate::bot::manifest_for;
use crate::plugin::CatalogEntry;
use crate::plugin::InstallError;
use crate::plugin::PluginError;
use crate::plugin::command::commands_from_manifest;
use crate::plugin::command::register_in_guild;
use crate::plugin::install;
use crate::update::PluginsCmd;
use crate::update::PluginsModel;
use crate::update::PluginsMsg;
use crate::update::PluginsUpdate;
use crate::update::Update;

/// Admin plugin management.
///
/// Lists the catalog and manages per-guild plugin enablement, registration,
/// and binary swaps. Requires server administrator permissions.
#[poise::command(slash_command, subcommands("list", "enable", "disable", "swap"))]
pub async fn plugins(ctx: Context<'_>) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    Ok(())
}

/// Lists every catalog plugin and its enabled state in this guild.
///
/// When the catalog failed to load, the bot owner sees the real path and
/// cause while other users keep the friendly empty message.
#[poise::command(slash_command)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    let model = guild_model(&data, guild_id).await?;

    if model.catalog.is_empty() {
        let reply =
            empty_catalog_message(data.plugin_catalog_error.as_ref(), author_is_bot_owner(ctx));
        ctx.say(reply).await?;
        return Ok(());
    }
    let mut lines = Vec::with_capacity(model.catalog.len());
    for name in &model.catalog {
        let state = if model.enabled.contains(name) {
            "enabled"
        } else {
            "disabled"
        };
        lines.push(format!("`{name}` — {state}"));
    }
    ctx.say(lines.join("\n")).await?;
    Ok(())
}

/// Enables a catalog plugin for this guild, registering the union of all
/// enabled plugins' commands.
#[poise::command(slash_command)]
pub async fn enable(ctx: Context<'_>, plugin: String) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    catalog_entry(&data, &plugin, author_is_bot_owner(ctx))?;
    let mut model = guild_model(&data, guild_id).await?;

    match PluginsUpdate::update(PluginsMsg::Enable(plugin.clone()), &mut model) {
        PluginsCmd::Register(_) => {
            register_enabled_plugins(
                &data.core_manifests,
                &data.plugin_catalog,
                &model.enabled,
                |commands| {
                    let http = ctx.serenity_context().http.clone();
                    Box::pin(async move {
                        register_in_guild(&http, &commands, guild_id)
                            .await
                            .map_err(Into::into)
                    })
                },
            )
            .await?;
            data.repos
                .guild_plugins()
                .set_enabled(guild_id.get(), &plugin, true)
                .await?;
            ctx.say(format!("Plugin `{plugin}` enabled.")).await?;
        }
        PluginsCmd::None => {
            ctx.say(format!("Plugin `{plugin}` is already enabled."))
                .await?;
        }
        _ => unreachable!("enable only ever registers or no-ops"),
    }
    Ok(())
}

/// Disables a plugin for this guild: unregisters the remaining enabled
/// plugins' commands.
#[poise::command(slash_command)]
pub async fn disable(ctx: Context<'_>, plugin: String) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    let mut model = guild_model(&data, guild_id).await?;

    match PluginsUpdate::update(PluginsMsg::Disable(plugin.clone()), &mut model) {
        PluginsCmd::Unregister(_) => {
            register_enabled_plugins(
                &data.core_manifests,
                &data.plugin_catalog,
                &model.enabled,
                |commands| {
                    let http = ctx.serenity_context().http.clone();
                    Box::pin(async move {
                        register_in_guild(&http, &commands, guild_id)
                            .await
                            .map_err(Into::into)
                    })
                },
            )
            .await?;
            // Persist the disabled state instead of deleting the row: an
            // absent row means auto-enabled, so a deletion would re-enable
            // the plugin on the next startup/join.
            data.repos
                .guild_plugins()
                .set_enabled(guild_id.get(), &plugin, false)
                .await?;
            // A plugin that is not running has nothing to unload; that is
            // not a failure for the disable path.
            if let Err(e) = data.plugin_manager.unload(&plugin, &[guild_id]).await
                && !matches!(e, PluginError::NotRunning { .. })
            {
                return Err(e.into());
            }
            ctx.say(format!("Plugin `{plugin}` disabled.")).await?;
        }
        PluginsCmd::None => {
            ctx.say(format!("Plugin `{plugin}` is not enabled."))
                .await?;
        }
        _ => unreachable!("disable only ever unregisters or no-ops"),
    }
    Ok(())
}

/// Swaps an enabled plugin to its freshly installed binary.
#[poise::command(slash_command)]
pub async fn swap(ctx: Context<'_>, plugin: String) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    let entry = catalog_entry(&data, &plugin, author_is_bot_owner(ctx))?;
    let mut model = guild_model(&data, guild_id).await?;

    match PluginsUpdate::update(PluginsMsg::Swap(plugin.clone()), &mut model) {
        PluginsCmd::Swap(_) => {
            let path = install::install_verified(
                &install::download_client(),
                entry,
                &data.config.plugins_dir,
            )
            .await?;
            data.plugin_manager
                .swap(&plugin, &path, &[guild_id])
                .await?;
            ctx.say(format!("Plugin `{plugin}` swapped.")).await?;
        }
        PluginsCmd::None => {
            ctx.say(format!("Plugin `{plugin}` is not enabled."))
                .await?;
        }
        _ => unreachable!("swap only ever swaps or no-ops"),
    }
    Ok(())
}

/// The catalog entry for `name`, or an error when unknown. When the catalog
/// failed to load, the bot owner's error names the real cause; other users
/// keep the plain unknown-plugin message.
fn catalog_entry<'a>(
    data: &'a Arc<crate::bot::Data>,
    name: &str,
    is_owner: bool,
) -> Result<&'a CatalogEntry, BotError> {
    data.plugin_catalog
        .get(name)
        .ok_or_else(|| BotError::InvalidCommandArgument {
            parameter: "plugin".to_string(),
            reason: unknown_plugin_reason(name, data.plugin_catalog_error.as_ref(), is_owner),
        })
}

/// The reason a plugin name is unknown: the plain message, or — for the bot
/// owner when the catalog failed to load — the real path and cause.
fn unknown_plugin_reason(
    name: &str,
    catalog_error: Option<&InstallError>,
    is_owner: bool,
) -> String {
    match catalog_error {
        Some(error) if is_owner => format!("`{name}` is not in the plugin catalog: {error}"),
        _ => format!("`{name}` is not in the plugin catalog"),
    }
}

/// The message for an empty catalog: the bot owner sees the real load
/// failure when there is one; other users keep the friendly message.
fn empty_catalog_message(catalog_error: Option<&InstallError>, is_owner: bool) -> String {
    match catalog_error {
        Some(error) if is_owner => format!("The plugin catalog is empty: {error}"),
        _ => "The plugin catalog is empty.".to_string(),
    }
}

/// The guild's plugins model: catalog names plus the guild's enabled subset.
///
/// Auto-enabled plugins (core plugins plus catalog entries flagged
/// `auto_enable`) are seeded into the enabled subset: an absent
/// `guild_plugins` row means enabled, so only an explicit `enabled = false`
/// row opts one out. This mirrors
/// [`register_plugins_in_guild`](crate::bot::BotEventHandler) and lets
/// `/plugins disable` turn an auto-enable off and persist `enabled = false`.
///
/// A listing failure propagates instead of masquerading as "no rows" —
/// the admin sees the real database error rather than wrong enablement
/// states (the startup path logs and skips; this path can reply).
async fn guild_model(
    data: &Arc<crate::bot::Data>,
    guild_id: GuildId,
) -> Result<PluginsModel, Error> {
    let catalog = data.plugin_catalog.keys().cloned().collect();
    let rows = data
        .repos
        .guild_plugins()
        .list_for_guild(guild_id.get())
        .await?;
    let mut enabled: Vec<String> = rows
        .iter()
        .filter(|entry| entry.enabled)
        .map(|entry| entry.plugin_name.clone())
        .collect();
    for plugin in data.auto_enable_plugins() {
        if !rows.iter().any(|entry| entry.plugin_name == plugin) {
            enabled.push(plugin);
        }
    }
    Ok(PluginsModel::new(catalog, enabled))
}

/// The routing commands of every plugin in `enabled`: the union a
/// `/plugins` toggle registers in one bulk call, so toggling one plugin
/// never erases another's registered commands. Plugins without a manifest
/// (a failed spawn with no catalog entry) contribute nothing. Sorted by
/// plugin name so the registered order is stable across restarts.
fn commands_for_enabled_plugins(
    core_manifests: &HashMap<String, Manifest>,
    catalog: &HashMap<String, CatalogEntry>,
    enabled: &[String],
) -> Vec<poise::Command<crate::bot::Data, Error>> {
    let mut commands = Vec::new();
    let mut enabled_names: Vec<&String> = enabled.iter().collect();
    enabled_names.sort();
    for name in enabled_names {
        let Some(manifest) = manifest_for(core_manifests, catalog, name) else {
            continue;
        };
        commands.extend(commands_from_manifest(manifest));
    }
    commands
}

/// Registers the union of every enabled plugin's routing commands via
/// `register` — one bulk call, so a `/plugins` toggle never erases another
/// plugin's commands. Production passes [`register_in_guild`]; tests pass a
/// recording closure so the wiring is asserted without a Discord
/// connection.
async fn register_enabled_plugins<F>(
    core_manifests: &HashMap<String, Manifest>,
    catalog: &HashMap<String, CatalogEntry>,
    enabled: &[String],
    register: F,
) -> Result<(), Error>
where
    F: FnOnce(
        Vec<poise::Command<crate::bot::Data, Error>>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'static>>,
{
    let commands = commands_for_enabled_plugins(core_manifests, catalog, enabled);
    register(commands).await
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_helpers::entry_named;
    use crate::test_helpers::manifest_named;

    /// The catalog load failure a fresh checkout produces: no `plugins.toml`.
    fn missing_catalog_error() -> InstallError {
        InstallError::Catalog {
            path: PathBuf::from("/srv/pwr-bot/data/plugins.toml"),
            detail: "No such file or directory (os error 2)".to_string(),
        }
    }

    #[test]
    fn empty_catalog_message_shows_the_load_failure_to_the_bot_owner() {
        let message = empty_catalog_message(Some(&missing_catalog_error()), true);

        assert!(
            message.contains("/srv/pwr-bot/data/plugins.toml"),
            "the owner sees the real path: {message}"
        );
        assert!(
            message.contains("No such file or directory"),
            "the owner sees the real cause: {message}"
        );
    }

    #[test]
    fn empty_catalog_message_keeps_the_friendly_line_for_other_users() {
        let message = empty_catalog_message(Some(&missing_catalog_error()), false);

        assert_eq!(message, "The plugin catalog is empty.");
    }

    #[test]
    fn empty_catalog_message_stays_friendly_when_the_catalog_is_genuinely_empty() {
        let message = empty_catalog_message(None, true);

        assert_eq!(message, "The plugin catalog is empty.");
    }

    #[test]
    fn unknown_plugin_reason_names_the_load_failure_for_the_bot_owner() {
        let reason = unknown_plugin_reason("hello", Some(&missing_catalog_error()), true);

        assert!(
            reason.contains("No such file or directory"),
            "the owner sees the real cause: {reason}"
        );
    }

    #[test]
    fn unknown_plugin_reason_keeps_the_plain_message_for_other_users() {
        let reason = unknown_plugin_reason("hello", Some(&missing_catalog_error()), false);

        assert_eq!(reason, "`hello` is not in the plugin catalog");
    }

    #[test]
    fn unknown_plugin_reason_keeps_the_plain_message_without_a_load_failure() {
        let reason = unknown_plugin_reason("hello", None, true);

        assert_eq!(reason, "`hello` is not in the plugin catalog");
    }

    #[test]
    fn enabling_one_plugin_registers_all_enabled_plugins_commands() {
        let core = HashMap::from([("settings".to_string(), manifest_named("settings"))]);
        let catalog = HashMap::from([("feed".to_string(), entry_named("feed"))]);

        let commands = commands_for_enabled_plugins(
            &core,
            &catalog,
            &["settings".to_string(), "feed".to_string()],
        );
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["feed", "settings"]);
    }

    #[test]
    fn disabling_one_plugin_registers_the_remaining_plugins_commands() {
        let core = HashMap::from([("settings".to_string(), manifest_named("settings"))]);
        let catalog = HashMap::from([("feed".to_string(), entry_named("feed"))]);

        // After `feed` is disabled, the remaining enabled set is just
        // `settings`; the re-registered union carries only its commands.
        let commands = commands_for_enabled_plugins(&core, &catalog, &["settings".to_string()]);
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["settings"]);
    }

    #[test]
    fn commands_for_enabled_plugins_skip_names_without_a_manifest() {
        let core = HashMap::from([("settings".to_string(), manifest_named("settings"))]);

        let commands = commands_for_enabled_plugins(
            &core,
            &HashMap::new(),
            &["ghost".to_string(), "settings".to_string()],
        );
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["settings"]);
    }

    #[tokio::test]
    async fn register_enabled_plugins_registers_the_union_of_all_enabled_plugins() {
        let core = HashMap::from([("settings".to_string(), manifest_named("settings"))]);
        let catalog = HashMap::from([("feed".to_string(), entry_named("feed"))]);

        let mut calls = 0;
        let mut registered: Vec<String> = Vec::new();
        let result = register_enabled_plugins(
            &core,
            &catalog,
            &["settings".to_string(), "feed".to_string()],
            |commands| {
                calls += 1;
                registered.extend(commands.iter().map(|command| command.name.to_string()));
                Box::pin(async { Ok(()) })
            },
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(calls, 1);
        assert_eq!(registered, ["feed", "settings"]);
    }

    #[tokio::test]
    async fn register_enabled_plugins_keeps_the_remaining_plugins_commands_after_a_disable() {
        let core = HashMap::from([("settings".to_string(), manifest_named("settings"))]);
        let catalog = HashMap::from([("feed".to_string(), entry_named("feed"))]);

        let mut registered: Vec<String> = Vec::new();
        let result =
            register_enabled_plugins(&core, &catalog, &["settings".to_string()], |commands| {
                registered.extend(commands.iter().map(|command| command.name.to_string()));
                Box::pin(async { Ok(()) })
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(registered, ["settings"]);
    }

    #[tokio::test]
    async fn register_enabled_plugins_with_no_enabled_plugins_registers_an_empty_set() {
        let core = HashMap::from([("settings".to_string(), manifest_named("settings"))]);
        let catalog = HashMap::from([("feed".to_string(), entry_named("feed"))]);

        let mut calls = 0;
        let mut registered: Vec<String> = Vec::new();
        let result = register_enabled_plugins(&core, &catalog, &[], |commands| {
            calls += 1;
            registered.extend(commands.iter().map(|command| command.name.to_string()));
            Box::pin(async { Ok(()) })
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(calls, 1);
        assert!(registered.is_empty());
    }
}
