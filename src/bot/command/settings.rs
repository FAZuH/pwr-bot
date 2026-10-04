//! The `/settings` command: the host Settings GUI.
//!
//! The command runs the Settings TEA feature on a Router session, listing
//! one tile per settings section the loaded plugin manifests declare for the
//! plugins this guild has enabled. A section click exits to
//! [`Navigation::SettingsSection`], which the Router resolves into the
//! Settings section handoff (see [`session_exit`]).
//!
//! `mode` names one of those sections up front, so `/settings feed` opens
//! the feed panel directly. Its choices are autocomplete rather than a fixed
//! `choices` list: a manifest that declares a settings panel does not have
//! to be a slash command, so nothing is synced to Discord when a plugin
//! gains or loses one — the same reason the plugin parameter on
//! `/plugin` autocompletes (see [`plugins::plugin_choices`]).
//!
//! A panel is only ever reached through this command, so every panel session
//! has a live Settings session waiting behind it: the panel's Back and About
//! hand the message back to a parked waiter (see
//! [`crate::bot::translate::SettingsReturns`]). A plugin that also published
//! its own panel slash command would open the panel with nobody waiting, and
//! its Back and About would have nowhere to return to.
use std::sync::Arc;
use std::time::Duration;

use log::warn;

use crate::bot::Data;
use crate::bot::Manifest;
use crate::bot::command::plugins::guild_model;
use crate::bot::command::prelude::*;
use crate::bot::gui::Host;
use crate::bot::gui::effects::NoopEffectHandler;
use crate::bot::gui::settings::SettingsConfig;
use crate::bot::gui::settings::SettingsFeature;
use crate::update::settings::SettingsEffect;
use crate::update::settings::SettingsMsg;
use crate::update::settings::SettingsSection;

/// Open the server settings menu, or one plugin's settings panel directly.
#[poise::command(slash_command, guild_only)]
pub async fn settings(
    ctx: Context<'_>,
    #[description = "A plugin's settings panel. Leave empty for this server's settings page."]
    #[autocomplete = "settings_mode"]
    mode: Option<String>,
) -> Result<(), Error> {
    // Every settings section is a plugin's panel and every panel write is
    // keyed by guild, so the whole command is guild-admin gated up front:
    // a member without server management is refused before the hub renders
    // and before any panel is handed a message. This is the same notion
    // `/plugin` enforces (`is_author_guild_admin`).
    is_author_guild_admin(ctx).await?;
    let coordinator = Router::new(ctx);
    let open = match mode
        .as_deref()
        .map(str::trim)
        .filter(|mode| !mode.is_empty())
    {
        None => None,
        Some(plugin) => Some(section_for(&coordinator, plugin).await?),
    };
    coordinator.open_section(open);
    invoke(coordinator).await
}

pub async fn invoke(coordinator: Arc<Router<'_>>) -> Result<(), Error> {
    coordinator.run(Navigation::SettingsMain).await?;
    Ok(())
}

/// The settings section `plugin` declares, or the refusal for a plugin that
/// declares none. Read from the sections this guild may open, never from a
/// host-side list — so a plugin this guild switched off has no panel to open.
async fn section_for(coordinator: &Router<'_>, plugin: &str) -> Result<SettingsSection, Error> {
    let sections = gather_sections(coordinator.context()).await;
    sections
        .into_iter()
        .find(|section| section.plugin == plugin)
        .ok_or_else(|| {
            BotError::InvalidCommandArgument {
                parameter: "mode".to_string(),
                reason: format!("`{plugin}` has no settings panel"),
            }
            .into()
        })
}

/// Discord's cap on choices per autocomplete response.
const MAX_AUTOCOMPLETE_CHOICES: usize = 25;

/// The `mode` choices: every plugin this guild may open a settings panel in,
/// by the plugin name the manifests key it under. Sorted, so the list a user
/// sees does not move between invocations, and matched as a prefix so a
/// partial name narrows it — the point of a choice, since the plugin ids are
/// not otherwise discoverable.
pub async fn settings_mode<'a>(ctx: Context<'a>, query: &'a str) -> CreateAutocompleteResponse<'a> {
    let names = mode_names(gather_sections(&ctx).await);
    let query = query.to_lowercase();
    let choices: Vec<AutocompleteChoice> = names
        .into_iter()
        .filter(|name| name.to_lowercase().starts_with(&query))
        .take(MAX_AUTOCOMPLETE_CHOICES)
        .map(AutocompleteChoice::from)
        .collect();
    CreateAutocompleteResponse::new().set_choices(choices)
}

/// The plugin names a `/settings mode` autocomplete offers: the sections this
/// guild may open, sorted and deduplicated by plugin.
fn mode_names(sections: Vec<SettingsSection>) -> Vec<String> {
    let mut names: Vec<String> = sections.into_iter().map(|section| section.plugin).collect();
    names.sort();
    names.dedup();
    names
}

handler! { pub struct SettingsHandler {} }

#[async_trait::async_trait]
impl CommandHandler for SettingsHandler {
    async fn run(&mut self, coordinator: Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        // A re-run after a section handoff wakes on an already-responded
        // interaction: the live reply exists, and only the first render
        // defers.
        if coordinator.reply_handle().await.is_none() {
            ctx.defer().await?;
        }

        let config = SettingsConfig {
            sections: gather_sections(&ctx).await,
        };

        let mut host = Host::<SettingsFeature, _>::new(
            ctx,
            config,
            NoopEffectHandler::<SettingsEffect, SettingsMsg>::new(),
            Duration::from_secs(120),
            coordinator.clone(),
        );

        // `/settings <mode>` names the panel that appears, so the session
        // renders one frame and exits into the handoff instead of running
        // the hub's event loop. The section rides the session (taken here,
        // once) rather than a handler field, so the hub's own entry point
        // stays the empty handler the rest of the Router builds.
        match coordinator.take_open_section() {
            Some(section) => {
                host.render_once().await?;
                coordinator
                    .navigate(Navigation::SettingsSection {
                        plugin: section.plugin,
                        command: section.command,
                    })
                    .await;
            }
            None => host.run().await?,
        }

        Ok(())
    }
}

/// Collects the settings sections this guild may open: the sections the
/// loaded plugin manifests declare (the same source set command registration
/// uses, internal plus catalog), ordered by plugin name so the list is stable
/// across restarts, narrowed to the plugins the guild has enabled.
///
/// The gate in [`session_exit::section_plugin`] already refuses a disabled
/// plugin at handoff time; filtering here is the half the user sees, so a
/// plugin this guild switched off is not offered a tile to press. A read
/// failure offers every declared section instead of none — the tile only ever
/// leads to the gate, which refuses with the real reason.
async fn gather_sections(ctx: &Context<'_>) -> Vec<SettingsSection> {
    let data = ctx.data();
    let declared = declared_sections(&data);
    let Some(guild_id) = ctx.guild_id() else {
        return declared;
    };
    match guild_model(&data, guild_id).await {
        Ok(model) => sections_for(declared, &model.enabled),
        Err(error) => {
            warn!("settings read no guild plugin rows, offering every section: {error}");
            declared
        }
    }
}

/// The settings sections `enabled` may open, in the order they were declared.
fn sections_for(declared: Vec<SettingsSection>, enabled: &[String]) -> Vec<SettingsSection> {
    declared
        .into_iter()
        .filter(|section| enabled.contains(&section.plugin))
        .collect()
}

/// Every settings section the loaded plugin manifests declare, over the same
/// sources as command registration, ordered by plugin name.
fn declared_sections(data: &Data) -> Vec<SettingsSection> {
    let mut manifests: Vec<(String, Manifest)> = data
        .internal_manifests
        .iter()
        .map(|(name, manifest)| (name.clone(), manifest.clone()))
        .collect();
    for (name, entry) in data.plugin_catalog.iter() {
        manifests.push((name.clone(), entry.manifest.clone()));
    }
    manifests.sort_by(|a, b| a.0.cmp(&b.0));
    manifests
        .into_iter()
        .flat_map(|(plugin, manifest)| {
            manifest
                .settings
                .into_iter()
                .map(move |section| SettingsSection::from((plugin.clone(), section)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section_for_plugin(plugin: &str) -> SettingsSection {
        SettingsSection {
            plugin: plugin.to_string(),
            command: format!("{plugin}-settings"),
            name: format!("{plugin} settings"),
            description: format!("the {plugin} panel"),
        }
    }

    /// A plugin this guild switched off has no tile to press: the sections are
    /// the manifests' panels narrowed to the enabled set, so `/settings`
    /// offers nothing for it. Fails if the list keeps reading manifests alone —
    /// the listing half of the gate the handoff now also runs.
    #[test]
    fn the_settings_list_omits_a_plugin_the_guild_switched_off() {
        let declared = vec![section_for_plugin("feed"), section_for_plugin("voice")];

        let sections = sections_for(declared, &["voice".to_string()]);

        let plugins: Vec<&str> = sections.iter().map(|s| s.plugin.as_str()).collect();
        assert_eq!(plugins, ["voice"], "the disabled plugin has no tile");
    }

    /// The `mode` autocomplete offers the same sections the list renders, so
    /// `/settings <plugin>` never suggests a panel the guild cannot open. A
    /// plugin with several sections is offered once.
    #[test]
    fn the_mode_autocomplete_offers_only_the_guilds_enabled_sections() {
        let declared = vec![
            section_for_plugin("feed"),
            section_for_plugin("voice"),
            section_for_plugin("voice"),
        ];

        let names = mode_names(sections_for(declared, &["voice".to_string()]));

        assert_eq!(names, ["voice".to_string()], "{names:?}");
    }
}
