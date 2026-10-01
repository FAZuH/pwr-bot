//! Admin plugin management commands: list, enable, disable, and swap plugins
//! from the external catalog.
//!
//! Subcommands are admin-gated (see
//! [`is_author_guild_admin`]) and
//! guild-scoped: enablement state lives in the `guild_plugins` table, and
//! command registration targets the invoking guild. Pure state transitions
//! run through [`PluginsUpdate`]; the returned
//! [`PluginsCmd`] drives the Discord/DB side
//! effects.

use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use log::debug;
use pwr_plugin_protocol::Manifest;

use crate::bot::add_plugin_commands;
use crate::bot::command::prelude::*;
use crate::bot::host_command_names;
use crate::bot::manifest_for;
use crate::bot::reply::text_reply;
use crate::plugin::CatalogEntry;
use crate::plugin::InstallError;
use crate::plugin::PluginError;
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
/// A catalog that carries no entries is a normal state, not a failure to
/// report: it answers through the shared error seam with a clean sentence and
/// leaves the load failure in the log.
#[poise::command(slash_command)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    let model = guild_model(&data, guild_id).await?;

    if model.catalog.is_empty() {
        return Err(empty_catalog_error(data.plugin_catalog_error.as_ref()));
    }
    ctx.send(text_reply(
        list_lines(&data.plugin_catalog, &model.enabled).join("\n"),
    ))
    .await?;
    Ok(())
}

/// The `/plugins list` body: one line per catalog plugin with its guild
/// state and the authority the operator granted it — the Discord token, or
/// the shaped `host.*` op surface alone.
fn list_lines(catalog: &HashMap<String, CatalogEntry>, enabled: &[String]) -> Vec<String> {
    let mut lines = Vec::with_capacity(catalog.len());
    for (name, entry) in catalog {
        let state = if enabled.contains(name) {
            "enabled"
        } else {
            "disabled"
        };
        let authority = if entry.discord_token {
            "discord token"
        } else {
            "host ops only"
        };
        lines.push(format!("`{name}` — {state}, {authority}"));
    }
    lines
}

/// Enables a catalog plugin for this guild, registering the union of all
/// enabled plugins' commands.
#[poise::command(slash_command)]
pub async fn enable(
    ctx: Context<'_>,
    #[description = "The catalog plugin to enable"]
    #[autocomplete = "plugin_choices"]
    plugin: String,
) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    catalog_entry(&data.plugin_catalog, &plugin)?;
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
            ctx.send(text_reply(format!("Plugin `{plugin}` enabled.")))
                .await?;
        }
        PluginsCmd::None => {
            ctx.send(text_reply(format!("Plugin `{plugin}` is already enabled.")))
                .await?;
        }
        _ => unreachable!("enable only ever registers or no-ops"),
    }
    Ok(())
}

/// Disables a plugin for this guild: unregisters the remaining enabled
/// plugins' commands.
#[poise::command(slash_command)]
pub async fn disable(
    ctx: Context<'_>,
    #[description = "The plugin to disable in this server"]
    #[autocomplete = "plugin_choices"]
    plugin: String,
) -> Result<(), Error> {
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
            ctx.send(text_reply(format!("Plugin `{plugin}` disabled.")))
                .await?;
        }
        PluginsCmd::None => {
            ctx.send(text_reply(format!("Plugin `{plugin}` is not enabled.")))
                .await?;
        }
        _ => unreachable!("disable only ever unregisters or no-ops"),
    }
    Ok(())
}

/// Swaps an enabled plugin to its freshly installed binary.
#[poise::command(slash_command)]
pub async fn swap(
    ctx: Context<'_>,
    #[description = "The enabled plugin to swap to a freshly installed binary"]
    #[autocomplete = "plugin_choices"]
    plugin: String,
) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    let entry = catalog_entry(&data.plugin_catalog, &plugin)?;
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
            ctx.send(text_reply(format!("Plugin `{plugin}` swapped.")))
                .await?;
        }
        PluginsCmd::None => {
            ctx.send(text_reply(format!("Plugin `{plugin}` is not enabled.")))
                .await?;
        }
        _ => unreachable!("swap only ever swaps or no-ops"),
    }
    Ok(())
}

/// The `plugin` choices for `/plugin enable|disable|swap`: every plugin the
/// host knows, so the ids are discoverable without reading a catalog file.
/// Both sources the host holds, always: the core manifests (the built-ins,
/// which `disable` can switch off) and the catalog (what `enable` and `swap`
/// resolve against). Never a list written here — a plugin that appears would
/// be invisible.
pub async fn plugin_choices<'a>(
    ctx: Context<'a>,
    query: &'a str,
) -> CreateAutocompleteResponse<'a> {
    let data = ctx.data();
    let mut names: Vec<String> = data
        .core_manifests
        .keys()
        .chain(data.plugin_catalog.keys())
        .cloned()
        .collect();
    names.sort();
    names.dedup();
    let query = query.to_lowercase();
    let choices: Vec<AutocompleteChoice> = names
        .into_iter()
        .filter(|name| name.to_lowercase().starts_with(&query))
        .take(MAX_AUTOCOMPLETE_CHOICES)
        .map(AutocompleteChoice::from)
        .collect();
    CreateAutocompleteResponse::new().set_choices(choices)
}

/// Discord's cap on choices per autocomplete response.
const MAX_AUTOCOMPLETE_CHOICES: usize = 25;

/// The catalog entry for `name`, or the refusal when the catalog does not
/// carry it. The catalog's own load failure is never spliced into the
/// message: an absent catalog is a normal state, and the path and IO cause
/// belong in the log, not in a user's reply.
fn catalog_entry<'a>(
    catalog: &'a HashMap<String, CatalogEntry>,
    name: &str,
) -> Result<&'a CatalogEntry, BotError> {
    catalog
        .get(name)
        .ok_or_else(|| BotError::InvalidCommandArgument {
            parameter: "plugin".to_string(),
            reason: format!("`{name}` is not in the plugin catalog"),
        })
}

/// The message for a catalog that carries no entries.
///
/// An absent catalog is a normal state — the operator has configured no
/// plugins — so it answers through the one error seam
/// ([`ErrorHandler::classify_error`]) with a clean sentence and no path, no
/// IO cause, and nothing doubled. The real cause stays in the log, where the
/// operator reads it: this function does not embed it.
fn empty_catalog_error(catalog_error: Option<&InstallError>) -> Error {
    if let Some(error) = catalog_error {
        debug!("the plugin catalog did not load, so there is nothing to list: {error}");
    }
    BotError::NoPluginCatalog.into()
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
    let mut registered_names = HashSet::new();
    let reserved_names = host_command_names();
    let mut enabled_names: Vec<&String> = enabled.iter().collect();
    enabled_names.sort();
    for name in enabled_names {
        let Some(manifest) = manifest_for(core_manifests, catalog, name) else {
            continue;
        };
        add_plugin_commands(
            &mut commands,
            &mut registered_names,
            manifest,
            &reserved_names,
        );
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

    use serde_json::json;

    use super::*;
    use crate::test_helpers::entry_named;
    use crate::test_helpers::manifest_named;

    fn manifest_with_command(plugin: &str, command: &str, description: &str) -> Manifest {
        let mut manifest = manifest_named(plugin);
        manifest.commands = vec![pwr_plugin_protocol::CommandDef {
            create_command: json!({
                "name": command,
                "description": description,
            }),
        }];
        manifest
    }

    /// The catalog load failure a fresh checkout produces: no `plugins.toml`.
    fn missing_catalog_error() -> InstallError {
        InstallError::Catalog {
            path: PathBuf::from("/srv/pwr-bot/data/plugins.toml"),
            detail: "No such file or directory (os error 2)".to_string(),
        }
    }

    /// An absent catalog answers cleanly through the shared seam. The path and
    /// the IO cause are in the log, not in the reply: a user has no
    /// `plugins.toml` to fix and no error to read. Fails if the load failure
    /// is spliced back into the message, or if the message arrives as bare
    /// text instead of an error the seam renders.
    #[test]
    fn an_absent_catalog_answers_through_the_error_seam_without_the_load_failure() {
        let error = empty_catalog_error(Some(&missing_catalog_error()));

        let bot_error = error
            .downcast_ref::<BotError>()
            .expect("the empty-catalog state answers through the error seam");
        assert!(
            matches!(bot_error, BotError::NoPluginCatalog),
            "the catalog-absent state is its own variant: {bot_error:?}"
        );
        let message = bot_error.to_string();
        assert!(
            !message.contains("plugins.toml"),
            "the path does not leak into a user-facing message: {message}"
        );
        assert!(
            !message.contains("os error"),
            "the IO cause does not leak into a user-facing message: {message}"
        );
        assert!(
            !message.contains("failed to load plugin catalog"),
            "the message is not the already-wrapped error, so nothing doubles: {message}"
        );
        let cause = missing_catalog_error().to_string();
        assert!(
            !message.contains(&cause),
            "the load failure is not spliced into the message, so nothing doubles: {message}"
        );
    }

    /// A catalog that parsed but holds nothing answers the same way, so the
    /// two empty states cannot render differently.
    #[test]
    fn a_genuinely_empty_catalog_answers_the_same_sentence() {
        let error = empty_catalog_error(None);

        let bot_error = error.downcast_ref::<BotError>().expect("seam");
        assert!(matches!(bot_error, BotError::NoPluginCatalog));
        assert_eq!(
            bot_error.to_string(),
            "No plugins are configured yet. The bot owner adds a plugin catalog to offer them."
        );
    }

    /// The unknown-plugin refusal never carries the catalog's load failure
    /// either: it is the same leak through the other door.
    #[test]
    fn an_unknown_plugin_name_never_carries_the_catalog_load_failure() {
        let reason = match catalog_entry(&HashMap::new(), "hello").expect_err("unknown") {
            BotError::InvalidCommandArgument { reason, .. } => reason,
            other => panic!("expected an argument refusal, got {other:?}"),
        };

        assert_eq!(reason, "`hello` is not in the plugin catalog");
    }

    #[test]
    fn list_lines_shows_each_plugins_state_and_authority() {
        let mut granted = entry_named("pro");
        granted.discord_token = true;
        let catalog = HashMap::from([
            ("hello".to_string(), entry_named("hello")),
            ("pro".to_string(), granted),
        ]);

        let lines = list_lines(&catalog, &["pro".to_string()]);

        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines.contains(&"`hello` — disabled, host ops only".to_string()),
            "{lines:?}"
        );
        assert!(
            lines.contains(&"`pro` — enabled, discord token".to_string()),
            "{lines:?}"
        );
    }

    #[test]
    fn enabling_one_plugin_registers_all_enabled_plugins_commands() {
        let core = HashMap::from([("voice".to_string(), manifest_named("voice"))]);
        let catalog = HashMap::from([("feed".to_string(), entry_named("feed"))]);

        let commands = commands_for_enabled_plugins(
            &core,
            &catalog,
            &["voice".to_string(), "feed".to_string()],
        );
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["feed", "voice"]);
    }

    #[test]
    fn disabling_one_plugin_registers_the_remaining_plugins_commands() {
        let core = HashMap::from([("voice".to_string(), manifest_named("voice"))]);
        let catalog = HashMap::from([("feed".to_string(), entry_named("feed"))]);

        // After `feed` is disabled, the remaining enabled set is just
        // `voice`; the re-registered union carries only its commands.
        let commands = commands_for_enabled_plugins(&core, &catalog, &["voice".to_string()]);
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["voice"]);
    }

    #[test]
    fn commands_for_enabled_plugins_skip_names_without_a_manifest() {
        let core = HashMap::from([("voice".to_string(), manifest_named("voice"))]);

        let commands = commands_for_enabled_plugins(
            &core,
            &HashMap::new(),
            &["ghost".to_string(), "voice".to_string()],
        );
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["voice"]);
    }

    #[test]
    fn manual_registration_reserves_host_roots_and_deduplicates_commands() {
        let core = HashMap::from([
            (
                "alpha".to_string(),
                manifest_with_command("alpha", "settings", "plugin settings"),
            ),
            (
                "beta".to_string(),
                manifest_with_command("beta", "shared", "first"),
            ),
        ]);
        let catalog = HashMap::from([(
            "gamma".to_string(),
            CatalogEntry {
                manifest: manifest_with_command("gamma", "shared", "second"),
                ..entry_named("gamma")
            },
        )]);

        let commands = commands_for_enabled_plugins(
            &core,
            &catalog,
            &["gamma".into(), "beta".into(), "alpha".into()],
        );
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["shared"]);
        assert_eq!(commands[0].description.as_deref(), Some("first"));
    }

    #[test]
    fn manual_registration_has_the_same_first_owner_for_every_input_order() {
        let core = HashMap::from([
            (
                "zeta".to_string(),
                manifest_with_command("zeta", "shared", "zeta"),
            ),
            (
                "alpha".to_string(),
                manifest_with_command("alpha", "shared", "alpha"),
            ),
        ]);
        let catalog = HashMap::new();

        let first = commands_for_enabled_plugins(&core, &catalog, &["zeta".into(), "alpha".into()]);
        let second =
            commands_for_enabled_plugins(&core, &catalog, &["alpha".into(), "zeta".into()]);

        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(first[0].description, second[0].description);
        assert_eq!(first[0].description.as_deref(), Some("alpha"));
    }

    #[tokio::test]
    async fn register_enabled_plugins_registers_the_union_of_all_enabled_plugins() {
        let core = HashMap::from([("voice".to_string(), manifest_named("voice"))]);
        let catalog = HashMap::from([("feed".to_string(), entry_named("feed"))]);

        let mut calls = 0;
        let mut registered: Vec<String> = Vec::new();
        let result = register_enabled_plugins(
            &core,
            &catalog,
            &["voice".to_string(), "feed".to_string()],
            |commands| {
                calls += 1;
                registered.extend(commands.iter().map(|command| command.name.to_string()));
                Box::pin(async { Ok(()) })
            },
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(calls, 1);
        assert_eq!(registered, ["feed", "voice"]);
    }

    #[tokio::test]
    async fn register_enabled_plugins_keeps_the_remaining_plugins_commands_after_a_disable() {
        let core = HashMap::from([("voice".to_string(), manifest_named("voice"))]);
        let catalog = HashMap::from([("feed".to_string(), entry_named("feed"))]);

        let mut registered: Vec<String> = Vec::new();
        let result =
            register_enabled_plugins(&core, &catalog, &["voice".to_string()], |commands| {
                registered.extend(commands.iter().map(|command| command.name.to_string()));
                Box::pin(async { Ok(()) })
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(registered, ["voice"]);
    }

    #[tokio::test]
    async fn register_enabled_plugins_with_no_enabled_plugins_registers_an_empty_set() {
        let core = HashMap::from([("voice".to_string(), manifest_named("voice"))]);
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
