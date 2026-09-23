//! The `/settings` command: the host Settings GUI.
//!
//! The command runs the Settings TEA feature on a Router session, listing
//! one tile per settings section the loaded plugin manifests declare. A
//! section click exits to [`Navigation::SettingsSection`], which the Router
//! resolves into the Settings section handoff (see [`session_exit`]).
use std::sync::Arc;
use std::time::Duration;

use crate::bot::Manifest;
use crate::bot::command::prelude::*;
use crate::bot::gui::Host;
use crate::bot::gui::effects::NoopEffectHandler;
use crate::bot::gui::settings::SettingsConfig;
use crate::bot::gui::settings::SettingsFeature;
use crate::update::settings::SettingsEffect;
use crate::update::settings::SettingsMsg;
use crate::update::settings::SettingsSection;

/// Open the server settings menu
#[poise::command(slash_command, guild_only)]
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    invoke(Router::new(ctx)).await
}

pub async fn invoke(coordinator: Arc<Router<'_>>) -> Result<(), Error> {
    coordinator.run(Navigation::SettingsMain).await?;
    Ok(())
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
            sections: gather_sections(&ctx),
        };

        let mut host = Host::<SettingsFeature, _>::new(
            ctx,
            config,
            NoopEffectHandler::<SettingsEffect, SettingsMsg>::new(),
            Duration::from_secs(120),
            coordinator.clone(),
        );

        host.run().await?;

        Ok(())
    }
}

/// Collects the settings sections the loaded plugin manifests declare —
/// the same source set command registration uses, core plus catalog —
/// ordered by plugin name so the list is stable across restarts.
fn gather_sections(ctx: &Context<'_>) -> Vec<SettingsSection> {
    let data = ctx.data();
    let mut manifests: Vec<(String, Manifest)> = data
        .core_manifests
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
