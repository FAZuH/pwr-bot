//! Admin plugin management commands: list, enable, disable, and swap plugins.
//!
//! A toggle takes any plugin the host knows — an internal plugin (feed, voice,
//! welcome) or a catalog entry — so [`plugin_choices`] and the lookups share
//! [`known_plugin_names`] as their single name list. `swap` is the exception:
//! it installs a freshly downloaded binary, so it resolves against catalog
//! entries only.
//!
//! Subcommands are admin-gated (see
//! [`is_author_guild_admin`]) and
//! guild-scoped: enablement state lives in the `guild_plugins` table, and
//! command registration targets the invoking guild. Pure state transitions
//! run through [`PluginsUpdate`]; the returned
//! [`PluginsCmd`] drives the Discord/DB side
//! effects.
//!
//! `list` is the one subcommand with a view: it runs the host
//! [`PluginsListFeature`] on a Router session and supplies the per-plugin line
//! wording ([`plugin_line`], shared by both groups), so the catalog group and
//! the internal group behind its Show/Hide Internal button render the same
//! states the toggles read. Its `show_internal` argument seeds that button's
//! state — hidden unless the admin asks — and the button toggles from there.
//!
//! Disabling an internal plugin gates it at the host for that guild and leaves the
//! shared process running for every other guild; only a catalog plugin, which
//! runs per guild, is unloaded.

use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use log::debug;
use log::warn;
use pwr_plugin_protocol::Manifest;

use crate::bot::Data;
use crate::bot::add_plugin_commands;
use crate::bot::command::prelude::*;
use crate::bot::gui::Host;
use crate::bot::gui::effects::NoopEffectHandler;
use crate::bot::gui::plugins::PluginsListConfig;
use crate::bot::gui::plugins::PluginsListFeature;
use crate::bot::host_command_names;
use crate::bot::manifest_for;
use crate::bot::reply::text_reply;
use crate::entity::GuildPluginEntity;
use crate::plugin::CatalogEntry;
use crate::plugin::PluginError;
use crate::plugin::command::register_in_guild;
use crate::plugin::install;
use crate::update::PluginsCmd;
use crate::update::PluginsListEffect;
use crate::update::PluginsListMsg;
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

/// Lists every plugin the host knows and its state in this guild; internal ones stay hidden.
///
/// The catalog group is always rendered; the internal group is left out unless
/// `show_internal` says True, and the view's Show/Hide Internal button toggles
/// from whichever state the argument set. Both groups are rendered by the host
/// [`PluginsListFeature`], whose per-plugin wording this module supplies. An
/// empty catalog renders its own group with `none configured` rather than
/// refusing: a group that disappeared would be indistinguishable from a
/// command that never knew about the catalog.
#[poise::command(slash_command)]
pub async fn list(
    ctx: Context<'_>,
    #[description = "Show the plugins that ship inside the bot"] show_internal: Option<bool>,
) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    invoke(Router::new(ctx), show_internal.unwrap_or(false)).await
}

pub async fn invoke(coordinator: Arc<Router<'_>>, show_internal: bool) -> Result<(), Error> {
    coordinator.show_internal(show_internal);
    coordinator.run(Navigation::PluginsList).await?;
    Ok(())
}

handler! { pub struct PluginsListHandler { show_internal: bool } }

#[async_trait::async_trait]
impl CommandHandler for PluginsListHandler {
    async fn run(&mut self, coordinator: Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        // A re-run after a section handoff wakes on an already-responded
        // interaction: the live reply exists, and only the first render
        // defers.
        if coordinator.reply_handle().await.is_none() {
            ctx.defer().await?;
        }

        let data = ctx.data();
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
        let model = guild_model(&data, guild_id).await?;

        let config = PluginsListConfig {
            internal: internal_lines(&data.internal_manifests, &model.enabled),
            catalog: list_lines(&data.plugin_catalog, &model.enabled),
            show_internal: self.show_internal,
        };
        // An absent catalog is a normal state the view renders as
        // `none configured`; the load failure behind it belongs in the log,
        // where the operator reads it, not in the group's sentence.
        if config.catalog.is_empty()
            && let Some(error) = data.plugin_catalog_error.as_ref()
        {
            debug!("the plugin catalog did not load, so the list has no catalog plugins: {error}");
        }

        let mut host = Host::<PluginsListFeature, _>::new(
            ctx,
            config,
            NoopEffectHandler::<PluginsListEffect, PluginsListMsg>::new(),
            Duration::from_secs(120),
            coordinator.clone(),
        );

        host.run().await?;
        Ok(())
    }
}

/// The `/plugins list` catalog group: one line per catalog plugin with its
/// guild state and the authority the operator granted it — the Discord token,
/// or the shaped `host.*` op surface alone.
///
/// Sorted by plugin name, so the rendered group does not move between renders.
fn list_lines(catalog: &HashMap<String, CatalogEntry>, enabled: &[String]) -> Vec<String> {
    let mut names: Vec<&String> = catalog.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let authority = if catalog[name].discord_token {
                DISCORD_TOKEN_AUTHORITY
            } else {
                HOST_OPS_AUTHORITY
            };
            plugin_line(name, enabled.contains(name), authority)
        })
        .collect()
}

/// The `/plugins list` internal group: the same line wording for the plugins
/// that ship inside this host binary. They hold the host's own Discord
/// authority — there is no separate binary, so there is no token grant to
/// withhold. Sorted by plugin name, like the catalog group.
fn internal_lines(
    internal_manifests: &HashMap<String, Manifest>,
    enabled: &[String],
) -> Vec<String> {
    let mut names: Vec<&String> = internal_manifests.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|name| plugin_line(name, enabled.contains(name), DISCORD_TOKEN_AUTHORITY))
        .collect()
}

/// One listed plugin: its state in this guild and the authority it holds.
/// Shared by both groups, so the wording exists once.
fn plugin_line(name: &str, enabled: bool, authority: &str) -> String {
    let state = if enabled { "enabled" } else { "disabled" };
    format!("`{name}` — {state}, {authority}")
}

/// The authority wording for a plugin the operator granted the Discord token.
const DISCORD_TOKEN_AUTHORITY: &str = "discord token";

/// The authority wording for a plugin held to the shaped `host.*` ops.
const HOST_OPS_AUTHORITY: &str = "host ops only";

/// Enables an internal or catalog plugin for this guild.
#[poise::command(slash_command)]
pub async fn enable(
    ctx: Context<'_>,
    #[description = "The plugin to enable"]
    #[autocomplete = "disabled_plugin_choices"]
    plugin: String,
) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    if manifest_for(&data.internal_manifests, &data.plugin_catalog, &plugin).is_none() {
        return Err(unknown_plugin(&plugin).into());
    }
    let mut model = guild_model(&data, guild_id).await?;

    match PluginsUpdate::update(PluginsMsg::Enable(plugin.clone()), &mut model) {
        PluginsCmd::Register(_) => {
            register_enabled_plugins(
                &data.internal_manifests,
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

/// Disables a plugin for this guild, unregistering the remaining commands.
#[poise::command(slash_command)]
pub async fn disable(
    ctx: Context<'_>,
    #[description = "The plugin to disable in this server"]
    #[autocomplete = "enabled_plugin_choices"]
    plugin: String,
) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
    let data = ctx.data();
    let mut model = guild_model(&data, guild_id).await?;

    match PluginsUpdate::update(PluginsMsg::Disable(plugin.clone()), &mut model) {
        PluginsCmd::Unregister(_) => {
            register_enabled_plugins(
                &data.internal_manifests,
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
            // Only a catalog plugin runs per guild, so only it can be
            // unloaded. An internal plugin is one process shared by every guild:
            // disabling it here gates it at the host for this guild only, and
            // the process keeps serving the others.
            if data.plugin_catalog.contains_key(&plugin)
                && let Err(e) = data.plugin_manager.unload(&plugin, &[guild_id]).await
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

/// Swaps an enabled catalog plugin to its freshly installed binary.
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
    let entry = swap_target(&data.internal_manifests, &data.plugin_catalog, &plugin)?;
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

/// The `plugin` choices for `/plugin swap`: every plugin the host knows, so
/// the ids are discoverable without reading a catalog file. `swap` offers
/// internal names too and refuses them with a reason, which beats hiding them.
///
/// Shares [`known_plugin_names`] with the toggle lookups, so an offered name
/// always resolves.
pub async fn plugin_choices<'a>(
    ctx: Context<'a>,
    query: &'a str,
) -> CreateAutocompleteResponse<'a> {
    choices_for(ctx, query, None).await
}

/// The `plugin` choices for `/plugin disable`: the plugins this guild has on,
/// and nothing else — a toggle's offer is what the command does next, and
/// `disable` acts on an enabled plugin.
pub async fn enabled_plugin_choices<'a>(
    ctx: Context<'a>,
    query: &'a str,
) -> CreateAutocompleteResponse<'a> {
    choices_for(ctx, query, Some(true)).await
}

/// The `plugin` choices for `/plugin enable`: the plugins this guild has off,
/// and nothing else — the mirror of [`enabled_plugin_choices`], so an
/// already-enabled plugin is not offered the toggle it would no-op.
pub async fn disabled_plugin_choices<'a>(
    ctx: Context<'a>,
    query: &'a str,
) -> CreateAutocompleteResponse<'a> {
    choices_for(ctx, query, Some(false)).await
}

/// The offered names, filtered to the guild's enabled set when the subcommand
/// acts on one (`want_enabled`), prefix-matched and capped.
///
/// A read failure offers every known plugin instead: the toggle commands
/// answer the offered name either way, and a list narrowed against a failed
/// read would hide the plugins the admin needs.
async fn choices_for<'a>(
    ctx: Context<'a>,
    query: &'a str,
    want_enabled: Option<bool>,
) -> CreateAutocompleteResponse<'a> {
    let data = ctx.data();
    let names = known_plugin_names(&data.internal_manifests, &data.plugin_catalog);
    let names = match (want_enabled, ctx.guild_id()) {
        (Some(want), Some(guild_id)) => match guild_model(&data, guild_id).await {
            Ok(model) => toggle_names(names, &model.enabled, want),
            Err(error) => {
                warn!("plugin autocomplete read no guild rows, offering every plugin: {error}");
                names
            }
        },
        _ => names,
    };
    let query = query.to_lowercase();
    let choices: Vec<AutocompleteChoice> = names
        .into_iter()
        .filter(|name| name.to_lowercase().starts_with(&query))
        .take(MAX_AUTOCOMPLETE_CHOICES)
        .map(AutocompleteChoice::from)
        .collect();
    CreateAutocompleteResponse::new().set_choices(choices)
}

/// The known names whose guild state is `want_enabled`: what `disable` offers
/// (the on ones) and what `enable` offers (the off ones). `enabled` is the
/// resolved set [`guild_model`] builds — auto-enabled plugins included — so a
/// name outside it really is off.
fn toggle_names(names: Vec<String>, enabled: &[String], want_enabled: bool) -> Vec<String> {
    names
        .into_iter()
        .filter(|name| enabled.contains(name) == want_enabled)
        .collect()
}

/// Discord's cap on choices per autocomplete response.
const MAX_AUTOCOMPLETE_CHOICES: usize = 25;

/// The catalog entry `name` swaps onto, or the refusal. A swap installs a
/// freshly downloaded binary from the catalog, so it resolves only against
/// catalog entries: an internal plugin ships inside this host binary and has
/// nothing to swap. The catalog's own load failure is never spliced into the
/// message — an absent catalog is a normal state, and the path and IO cause
/// belong in the log, not in a user's reply.
fn swap_target<'a>(
    internal_manifests: &HashMap<String, Manifest>,
    catalog: &'a HashMap<String, CatalogEntry>,
    name: &str,
) -> Result<&'a CatalogEntry, BotError> {
    if internal_manifests.contains_key(name) {
        return Err(BotError::InvalidCommandArgument {
            parameter: "plugin".to_string(),
            reason: format!(
                "`{name}` is an internal plugin: it ships with the bot, so there is \
                 nothing to swap"
            ),
        });
    }
    catalog.get(name).ok_or_else(|| unknown_plugin(name))
}

/// The refusal for a plugin name the host knows no source for.
fn unknown_plugin(name: &str) -> BotError {
    BotError::InvalidCommandArgument {
        parameter: "plugin".to_string(),
        reason: format!("`{name}` is not a known plugin"),
    }
}

/// Every plugin the host knows, sorted and deduplicated: the internal manifests
/// plus the catalog. The two sources `/plugin enable|disable|swap` resolve
/// against, so both offer the same names.
fn known_plugin_names(
    internal_manifests: &HashMap<String, Manifest>,
    catalog: &HashMap<String, CatalogEntry>,
) -> Vec<String> {
    let mut names: Vec<String> = internal_manifests
        .keys()
        .chain(catalog.keys())
        .cloned()
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The guild's plugins model: the names the host knows plus the guild's
/// enabled subset, read from that guild's `guild_plugins` rows.
///
/// Auto-enabled plugins (internal plugins plus catalog entries flagged
/// `auto_enable`) are seeded into the enabled subset: an absent
/// `guild_plugins` row means enabled, so only an explicit `enabled = false`
/// row opts one out. This mirrors
/// [`register_plugins_in_guild`](crate::bot::BotEventHandler) and lets
/// `/plugins disable` turn an auto-enable off and persist `enabled = false`.
///
/// A listing failure propagates instead of masquerading as "no rows" —
/// the admin sees the real database error rather than wrong enablement
/// states (the startup path logs and skips; this path can reply).
pub(crate) async fn guild_model(data: &Data, guild_id: GuildId) -> Result<PluginsModel, Error> {
    let rows = data
        .repos
        .guild_plugins()
        .list_for_guild(guild_id.get())
        .await?;
    Ok(guild_plugins_model(
        &data.internal_manifests,
        &data.plugin_catalog,
        &data.auto_enable_plugins(),
        &rows,
    ))
}

impl Data {
    /// Whether `name` is enabled for `guild_id`, read through the same
    /// enabled-set resolution `/plugins` toggles and lists with: a plugin with
    /// no row is auto-enabled, an `enabled = false` row switches it off.
    ///
    /// The serving path asks this before invoking a plugin, so a per-guild
    /// disable stops a shared internal plugin's process serving that guild
    /// without unloading it. A `None` guild is a DM: there is no per-guild
    /// state to gate, so the plugin is served.
    ///
    /// A listing failure propagates rather than answering "off", so a caller
    /// that cannot answer has to decide what to do with it.
    pub async fn plugin_enabled_in(
        &self,
        guild_id: Option<GuildId>,
        name: &str,
    ) -> Result<bool, Error> {
        let Some(guild_id) = guild_id else {
            return Ok(true);
        };
        Ok(guild_model(self, guild_id)
            .await?
            .enabled
            .iter()
            .any(|plugin| plugin == name))
    }

    /// The plugin owning `author`'s pending modal submission when that plugin
    /// is disabled in `guild_id` — consuming the route — else `None`.
    ///
    /// A modal opened before the toggle would otherwise deliver its write
    /// after it: the submission half of the per-guild gate, beside the
    /// command and click gates. Consuming keeps the refusal one-shot (the
    /// route is the one-shot, not the delivery), so a retried submit finds
    /// nothing. A `None` guild (DM), no route, or an enabled owner is `None`
    /// and falls through to the delivery unchanged.
    ///
    /// A row read that fails serves the submission anyway, matching
    /// [`Data::plugin_enabled_in`]'s callers: a database blip must not eat a
    /// modal the user is waiting on.
    pub async fn take_disabled_modal(
        &self,
        author: u64,
        guild_id: Option<GuildId>,
    ) -> Option<String> {
        let binding = self.plugin_manager.peek_modal(author).await.ok()?;
        match self.plugin_enabled_in(guild_id, &binding.owner).await {
            Ok(false) => {
                if let Err(e) = self.plugin_manager.take_modal(author).await {
                    debug!(
                        "no modal route left to consume for `{}`: {e}",
                        binding.owner
                    );
                }
                Some(binding.owner)
            }
            Ok(true) => None,
            Err(e) => {
                warn!(
                    "failed to read the guild plugin rows for `{}`: {e}",
                    binding.owner
                );
                None
            }
        }
    }
}

/// The guild's plugins model over the host's two sources and the guild's
/// `guild_plugins` rows. Both sources count as known: an internal plugin name is
/// as toggleable as a catalog one, so it has to reach the model or
/// [`PluginsUpdate`] reads it as unknown and refuses the toggle.
fn guild_plugins_model(
    internal_manifests: &HashMap<String, Manifest>,
    catalog: &HashMap<String, CatalogEntry>,
    auto_enable: &[String],
    rows: &[GuildPluginEntity],
) -> PluginsModel {
    let mut enabled: Vec<String> = rows
        .iter()
        .filter(|row| row.enabled)
        .map(|row| row.plugin_name.clone())
        .collect();
    for plugin in auto_enable {
        if !rows.iter().any(|row| row.plugin_name == *plugin) {
            enabled.push(plugin.clone());
        }
    }
    PluginsModel::new(known_plugin_names(internal_manifests, catalog), enabled)
}

/// The routing commands of every plugin in `enabled`: the union a
/// `/plugins` toggle registers in one bulk call, so toggling one plugin
/// never erases another's registered commands. Plugins without a manifest
/// (a failed spawn with no catalog entry) contribute nothing. Sorted by
/// plugin name so the registered order is stable across restarts.
fn commands_for_enabled_plugins(
    internal_manifests: &HashMap<String, Manifest>,
    catalog: &HashMap<String, CatalogEntry>,
    enabled: &[String],
) -> Vec<poise::Command<Data, Error>> {
    let mut commands = Vec::new();
    let mut registered_names = HashSet::new();
    let reserved_names = host_command_names();
    let mut enabled_names: Vec<&String> = enabled.iter().collect();
    enabled_names.sort();
    for name in enabled_names {
        let Some(manifest) = manifest_for(internal_manifests, catalog, name) else {
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
    internal_manifests: &HashMap<String, Manifest>,
    catalog: &HashMap<String, CatalogEntry>,
    enabled: &[String],
    register: F,
) -> Result<(), Error>
where
    F: FnOnce(
        Vec<poise::Command<Data, Error>>,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'static>>,
{
    let commands = commands_for_enabled_plugins(internal_manifests, catalog, enabled);
    register(commands).await
}

#[cfg(test)]
mod tests {
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

    /// The unknown-plugin refusal never carries the catalog's load failure
    /// either: it is the same leak through the other door. An internal plugin is
    /// not unknown, so `swap` names the real reason instead.
    #[test]
    fn an_unknown_plugin_name_never_carries_the_catalog_load_failure() {
        let reason =
            match swap_target(&HashMap::new(), &HashMap::new(), "hello").expect_err("unknown") {
                BotError::InvalidCommandArgument { reason, .. } => reason,
                other => panic!("expected an argument refusal, got {other:?}"),
            };

        assert_eq!(reason, "`hello` is not a known plugin");
    }

    /// An internal plugin has no catalog URL and no separate binary, so `swap`
    /// cannot serve it: the refusal says why instead of claiming the plugin
    /// does not exist. Fails if the internal refusal reads like the unknown one.
    #[test]
    fn swapping_a_internal_plugin_is_refused_because_it_ships_with_the_bot() {
        let core = HashMap::from([("feed".to_string(), manifest_named("feed"))]);

        let reason = match swap_target(&core, &HashMap::new(), "feed").expect_err("core") {
            BotError::InvalidCommandArgument { reason, .. } => reason,
            other => panic!("expected an argument refusal, got {other:?}"),
        };

        assert_eq!(
            reason,
            "`feed` is an internal plugin: it ships with the bot, so there is nothing \
                          to swap"
        );
    }

    /// A catalog plugin still swaps onto its entry, internal manifests present or
    /// not: the refusal is scoped to the internal name, not to the command.
    #[test]
    fn swap_still_targets_a_catalog_plugin_alongside_a_core_one() {
        let core = HashMap::from([("feed".to_string(), manifest_named("feed"))]);
        let catalog = HashMap::from([("hello".to_string(), entry_named("hello"))]);

        let entry = swap_target(&core, &catalog, "hello").expect("catalog plugin swaps");

        assert_eq!(entry.name, "hello");
    }

    /// The names `/plugin enable` accepts are the host's two sources: an internal
    /// plugin is as enableable as a catalog one, so the offered names must
    /// resolve. Fails if `enable` still gates on the catalog alone.
    #[test]
    fn enable_accepts_a_internal_plugin_name_the_host_knows() {
        let core = HashMap::from([("feed".to_string(), manifest_named("feed"))]);
        let catalog = HashMap::new();

        assert!(
            manifest_for(&core, &catalog, "feed").is_some(),
            "an internal plugin resolves over the same lookup the toggle guards with"
        );
        assert!(
            manifest_for(&core, &catalog, "nope").is_none(),
            "an unknown name stays unknown"
        );
    }

    /// The offered names and the toggle's known names are one list, so
    /// `/plugin` autocomplete cannot suggest a name the toggle then rejects.
    #[test]
    fn known_plugin_names_carries_both_sources_once() {
        let core = HashMap::from([
            ("feed".to_string(), manifest_named("feed")),
            ("voice".to_string(), manifest_named("voice")),
        ]);
        // `feed` is in both sources: a catalog entry for an internal plugin must
        // not produce a duplicate choice.
        let catalog = HashMap::from([("feed".to_string(), entry_named("feed"))]);

        let names = known_plugin_names(&core, &catalog);

        assert_eq!(names, ["feed", "voice"]);
    }

    /// A `guild_plugins` row round-trips an internal plugin's state through the
    /// toggle: `disable` writes `enabled = false`, the next model build keeps
    /// the internal plugin out, `enable` registers it again, and the rebuild from
    /// the `enabled = true` row has it back. Fails if the model knows only
    /// catalog names (the internal plugin never enables) or ignores the row (a
    /// disabled internal plugin re-enables itself).
    #[test]
    fn a_internal_plugin_toggles_and_round_trips_through_its_guild_plugin_row() {
        let core = HashMap::from([("feed".to_string(), manifest_named("feed"))]);
        let catalog = HashMap::new();
        let auto_enable = vec!["feed".to_string()];
        let row = |enabled: bool| GuildPluginEntity {
            guild_id: 7.into(),
            plugin_name: "feed".to_string(),
            enabled,
        };

        // No row yet: the auto-enabled internal plugin is in the model, and the
        // toggle knows its name.
        let mut model = guild_plugins_model(&core, &catalog, &auto_enable, &[]);
        assert!(model.catalog.contains(&"feed".to_string()));
        assert!(model.enabled.contains(&"feed".to_string()));

        // `/plugin disable feed`: unregisters and persists enabled = false.
        assert_eq!(
            PluginsUpdate::update(PluginsMsg::Disable("feed".into()), &mut model),
            PluginsCmd::Unregister("feed".into())
        );
        let model = guild_plugins_model(&core, &catalog, &auto_enable, &[row(false)]);
        assert!(
            !model.enabled.contains(&"feed".to_string()),
            "the enabled = false row keeps the internal plugin off"
        );

        // `/plugin enable feed` registers again, and the enabled = true row
        // brings it back on the next build.
        let mut model = guild_plugins_model(&core, &catalog, &auto_enable, &[row(false)]);
        assert_eq!(
            PluginsUpdate::update(PluginsMsg::Enable("feed".into()), &mut model),
            PluginsCmd::Register("feed".into())
        );
        let model = guild_plugins_model(&core, &catalog, &auto_enable, &[row(true)]);
        assert!(model.enabled.contains(&"feed".to_string()));
    }

    /// The re-registered command union follows the row: an internal plugin left
    /// out of the guild's enabled set contributes no commands, which is what
    /// makes the disable gate real in that guild.
    #[test]
    fn a_disabled_internal_plugin_contributes_no_commands_to_the_guild() {
        let core = HashMap::from([
            ("feed".to_string(), manifest_named("feed")),
            ("voice".to_string(), manifest_named("voice")),
        ]);
        let catalog = HashMap::new();
        let model = guild_plugins_model(
            &core,
            &catalog,
            &["feed".to_string(), "voice".to_string()],
            &[GuildPluginEntity {
                guild_id: 7.into(),
                plugin_name: "feed".to_string(),
                enabled: false,
            }],
        );

        let commands = commands_for_enabled_plugins(&core, &catalog, &model.enabled);
        let names: Vec<&str> = commands
            .iter()
            .map(|command| command.name.as_ref())
            .collect();

        assert_eq!(names, ["voice"]);
    }

    /// An empty catalog is a group with no lines, not a refusal: the view
    /// renders it as `none configured`, which an omitted group could not be
    /// mistaken for. Internal plugins are the case that made the old empty-body
    /// 400 reachable — they are known and toggleable without being catalog
    /// entries, so an empty catalog beside an internal plugin is exactly the
    /// state that has to render something. Fails if the emptiness is read from
    /// the known names, where that state reads as a full catalog.
    #[test]
    fn an_empty_catalog_yields_an_empty_group_rather_than_a_refusal() {
        let internal = HashMap::from([("feed".to_string(), manifest_named("feed"))]);
        let known = known_plugin_names(&internal, &HashMap::new());
        assert!(
            !known.is_empty(),
            "the guild's known names carry the internal plugin, which is not a catalog entry"
        );

        assert!(
            list_lines(&HashMap::new(), &known).is_empty(),
            "no catalog plugins means no catalog lines, and the view says so itself"
        );
        assert_eq!(
            internal_lines(&internal, &known),
            ["`feed` — enabled, discord token"],
            "the internal group is unaffected by an empty catalog"
        );
    }

    /// Both groups are built by the same wording helper, so a plugin reads the
    /// same in either group and the two lists cannot drift apart.
    #[test]
    fn both_groups_share_one_line_wording() {
        assert_eq!(
            plugin_line("feed", true, DISCORD_TOKEN_AUTHORITY),
            "`feed` — enabled, discord token"
        );
        assert_eq!(
            plugin_line("hello", false, HOST_OPS_AUTHORITY),
            "`hello` — disabled, host ops only"
        );
    }

    /// The internal group carries each internal plugin's guild state under the
    /// shared wording, so the Show/Hide button reveals the same information the
    /// catalog group shows rather than a second, differently-worded list.
    #[test]
    fn the_internal_group_shows_each_plugins_state_under_the_shared_wording() {
        let internal = HashMap::from([
            ("feed".to_string(), manifest_named("feed")),
            ("voice".to_string(), manifest_named("voice")),
        ]);

        let lines = internal_lines(&internal, &["feed".to_string()]);

        assert_eq!(
            lines,
            [
                "`feed` — enabled, discord token",
                "`voice` — disabled, discord token"
            ]
        );
    }

    /// Both groups render in plugin-name order whatever the map's iteration
    /// order is: a view whose rows reshuffle between renders reads as noise.
    #[test]
    fn both_groups_are_sorted_by_plugin_name() {
        let internal = HashMap::from([
            ("zebra".to_string(), manifest_named("zebra")),
            ("alpha".to_string(), manifest_named("alpha")),
            ("mango".to_string(), manifest_named("mango")),
        ]);
        let catalog = HashMap::from([
            ("yak".to_string(), entry_named("yak")),
            ("bee".to_string(), entry_named("bee")),
            ("cat".to_string(), entry_named("cat")),
        ]);

        assert_eq!(
            internal_lines(&internal, &[]),
            [
                "`alpha` — disabled, discord token",
                "`mango` — disabled, discord token",
                "`zebra` — disabled, discord token",
            ]
        );
        assert_eq!(
            list_lines(&catalog, &[]),
            [
                "`bee` — disabled, host ops only",
                "`cat` — disabled, host ops only",
                "`yak` — disabled, host ops only",
            ]
        );
    }
    /// Each toggle offers what it acts on: `disable` the plugins this guild
    /// has on, `enable` the ones it has off. Fails if both subcommands share
    /// one unfiltered list, so an admin is offered a toggle that no-ops.
    #[test]
    fn the_toggle_autocompletes_offer_each_plugin_on_the_side_the_command_acts_on() {
        let names = vec!["feed".to_string(), "voice".to_string()];
        let enabled = vec!["feed".to_string()];

        assert_eq!(
            toggle_names(names.clone(), &enabled, true),
            ["feed".to_string()],
            "`disable` offers the enabled plugins"
        );
        assert_eq!(
            toggle_names(names, &enabled, false),
            ["voice".to_string()],
            "`enable` offers the disabled plugins"
        );
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
