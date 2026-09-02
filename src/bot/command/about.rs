//! About command showing bot statistics and information.
use std::sync::Arc;
use std::time::Duration;

use chrono::Datelike;
use chrono::Utc;
use poise::Command;

use crate::bot::command::prelude::*;
use crate::bot::gui::Host;
use crate::bot::gui::about::AboutConfig;
use crate::bot::gui::about::AboutFeature;
use crate::bot::gui::effects::NoopEffectHandler;
use crate::update::about::AboutEffect;
use crate::update::about::AboutMsg;
use crate::update::about::AboutStats;

/// Show information about the bot
#[poise::command(slash_command)]
pub async fn about(ctx: Context<'_>) -> Result<(), Error> {
    invoke(Router::new(ctx)).await
}

pub async fn invoke(coordinator: Arc<Router<'_>>) -> Result<(), Error> {
    coordinator.run(Navigation::SettingsAbout).await?;
    Ok(())
}

handler! { pub struct AboutHandler<'a> {} }

#[async_trait::async_trait]
impl CommandHandler for AboutHandler<'_> {
    async fn run(&mut self, coordinator: Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let stats = AboutStats::gather_stats(&ctx).await?;
        let avatar_url = ctx.cache().current_user().face();
        let config = AboutConfig { stats, avatar_url };

        let mut host = Host::<AboutFeature, _>::new(
            ctx,
            config,
            NoopEffectHandler::<AboutEffect, AboutMsg>::new(),
            Duration::from_secs(120),
            coordinator.clone(),
        );

        host.run().await?;

        Ok(())
    }
}

impl AboutStats {
    /// Gathers bot statistics for the about command.
    pub(crate) async fn gather_stats(ctx: &Context<'_>) -> Result<AboutStats, Error> {
        let start_time = ctx.data().start_time;
        let version = ctx.data().config.version.clone();
        let uptime = start_time.elapsed();

        let guild_count = Context::cache(*ctx).guilds().len();

        let user_count: usize = Context::cache(*ctx)
            .guilds()
            .iter()
            .filter_map(|guild_id| {
                Context::cache(*ctx)
                    .guild(*guild_id)
                    .map(|guild| guild.member_count.get() as usize)
            })
            .sum();

        // Make a request to Discord server to get latency
        let latency_start = std::time::Instant::now();
        let _ = ctx.http().get_current_user().await?;
        let latency_ms = latency_start.elapsed().as_millis() as u64;

        let command_count = Self::count_commands(&ctx.framework().options().commands);

        let memory_mb = Self::get_process_memory_mb();

        let current_year = Utc::now().year();

        Ok(AboutStats::new(
            version,
            uptime,
            guild_count,
            user_count,
            latency_ms,
            command_count,
            memory_mb,
            current_year,
        ))
    }

    fn count_commands<U, E>(commands: &[Command<U, E>]) -> usize {
        commands
            .iter()
            .map(|cmd| 1 + Self::count_commands(&cmd.subcommands))
            .sum()
    }

    /// Gets the current process memory usage in megabytes.
    fn get_process_memory_mb() -> f64 {
        use sysinfo::System;
        use sysinfo::get_current_pid;

        let mut s = System::new_all();
        s.refresh_all();

        if let Ok(pid) = get_current_pid()
            && let Some(process) = s.process(pid)
        {
            return process.memory() as f64 / (1024.0 * 1024.0);
        }

        0.0
    }
}
