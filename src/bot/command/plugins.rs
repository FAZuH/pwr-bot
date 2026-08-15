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

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::plugin::CatalogEntry;
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
#[poise::command(slash_command)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    let model = guild_model(&data, guild_id).await;

    if model.catalog.is_empty() {
        ctx.say("The plugin catalog is empty.").await?;
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

/// Enables a catalog plugin for this guild: registers its commands and marks
/// it enabled.
#[poise::command(slash_command)]
pub async fn enable(ctx: Context<'_>, plugin: String) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    let entry = catalog_entry(&data, &plugin)?;
    let mut model = guild_model(&data, guild_id).await;

    match PluginsUpdate::update(PluginsMsg::Enable(plugin.clone()), &mut model) {
        PluginsCmd::Register(_) => {
            let commands = commands_from_manifest(&entry.manifest);
            register_in_guild(ctx.http(), &commands, guild_id).await?;
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

/// Disables a plugin for this guild: unregisters and unloads it.
#[poise::command(slash_command)]
pub async fn disable(ctx: Context<'_>, plugin: String) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    let mut model = guild_model(&data, guild_id).await;

    match PluginsUpdate::update(PluginsMsg::Disable(plugin.clone()), &mut model) {
        PluginsCmd::Unregister(_) => {
            register_in_guild(ctx.http(), &[], guild_id).await?;
            data.repos
                .guild_plugins()
                .delete(guild_id.get(), &plugin)
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
    let entry = catalog_entry(&data, &plugin)?;
    let mut model = guild_model(&data, guild_id).await;

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

/// The catalog entry for `name`, or an error when unknown.
fn catalog_entry<'a>(
    data: &'a Arc<crate::bot::Data>,
    name: &str,
) -> Result<&'a CatalogEntry, BotError> {
    data.plugin_catalog
        .get(name)
        .ok_or_else(|| BotError::InvalidCommandArgument {
            parameter: "plugin".to_string(),
            reason: format!("`{name}` is not in the plugin catalog"),
        })
}

/// The guild's plugins model: catalog names plus the guild's enabled subset.
async fn guild_model(data: &Arc<crate::bot::Data>, guild_id: GuildId) -> PluginsModel {
    let catalog = data.plugin_catalog.keys().cloned().collect();
    let enabled = data
        .repos
        .guild_plugins()
        .list_for_guild(guild_id.get())
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.enabled)
        .map(|entry| entry.plugin_name)
        .collect();
    PluginsModel::new(catalog, enabled)
}
