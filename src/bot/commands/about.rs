use std::borrow::Cow;
use std::time::Duration;

use poise::Command;
use poise::serenity_prelude::CreateActionRow;
use poise::serenity_prelude::CreateButton;
use poise::serenity_prelude::CreateComponent;
use poise::serenity_prelude::CreateContainer;
use poise::serenity_prelude::CreateContainerComponent;
use poise::serenity_prelude::CreateSection;
use poise::serenity_prelude::CreateSectionAccessory;
use poise::serenity_prelude::CreateSectionComponent;
use poise::serenity_prelude::CreateTextDisplay;
use poise::serenity_prelude::CreateThumbnail;
use poise::serenity_prelude::CreateUnfurledMediaItem;
use sysinfo::System;

use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::views::ResponseComponentView;
use crate::bot::views::ViewProvider;

pub struct AboutCog;

impl AboutCog {
    /// Show information about the bot
    #[poise::command(slash_command)]
    pub async fn about(ctx: Context<'_>) -> Result<(), Error> {
        ctx.defer().await?;

        let stats = gather_stats(&ctx).await?;
        let avatar_url = ctx.cache().current_user().face();
        let view = AboutView::new(stats, avatar_url);

        ctx.send(view.create_reply()).await?;

        Ok(())
    }
}

impl Cog for AboutCog {
    fn commands(&self) -> Vec<Command<crate::bot::Data, Error>> {
        vec![Self::about()]
    }
}

struct AboutStats {
    version: String,
    uptime: Duration,
    guild_count: usize,
    user_count: usize,
    latency_ms: u64,
    command_count: usize,
    memory_mb: f64,
}

struct AboutView {
    stats: AboutStats,
    avatar_url: String,
}

impl AboutView {
    fn new(stats: AboutStats, avatar_url: String) -> Self {
        Self { stats, avatar_url }
    }

    fn format_uptime(duration: Duration) -> String {
        let days = duration.as_secs() / 86400;
        let hours = (duration.as_secs() % 86400) / 3600;
        let minutes = (duration.as_secs() % 3600) / 60;

        if days > 0 {
            format!("{} days, {} hours, {} minutes", days, hours, minutes)
        } else if hours > 0 {
            format!("{} hours, {} minutes", hours, minutes)
        } else {
            format!("{} minutes", minutes)
        }
    }

    fn format_number(num: usize) -> String {
        if num >= 1_000_000 {
            format!("{:.1}M", num as f64 / 1_000_000.0)
        } else if num >= 1_000 {
            format!("{:.1}k", num as f64 / 1_000.0)
        } else {
            num.to_string()
        }
    }
}

impl<'a> ViewProvider<'a> for AboutView {
    fn create(&self) -> Vec<CreateComponent<'a>> {
        let title_text = format!("## pwr-bot\n\n**Version:** {}", self.stats.version);

        let stats_text = format!(
            "### Stats\n\nUptime: {}\nServers: {}\nUsers: {}\nLatency: {}ms\nCommands: {}\nMemory: {:.1} MB",
            Self::format_uptime(self.stats.uptime),
            Self::format_number(self.stats.guild_count),
            Self::format_number(self.stats.user_count),
            self.stats.latency_ms,
            self.stats.command_count,
            self.stats.memory_mb
        );

        let info_text = "### Info\n\nAuthor: [FAZuH](https://github.com/FAZuH)\nSource: [GitHub](https://github.com/FAZuH/pwr-bot)\nLicense: MIT";

        let avatar_url: String = self.avatar_url.clone();
        let avatar = CreateThumbnail::new(CreateUnfurledMediaItem::new(avatar_url));

        let title_section = CreateSection::new(
            vec![CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                title_text,
            ))],
            CreateSectionAccessory::Thumbnail(avatar),
        );

        let github_button =
            CreateButton::new_link("https://github.com/FAZuH/pwr-bot").label("GitHub");

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::Section(title_section),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(stats_text)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(info_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::Buttons(Cow::Owned(vec![
                github_button,
            ]))),
        ]));

        vec![container]
    }
}

impl ResponseComponentView for AboutView {}

async fn gather_stats(ctx: &Context<'_>) -> Result<AboutStats, Error> {
    let start_time = ctx.data().start_time;
    let uptime = start_time.elapsed();

    let guild_count = ctx.cache().guilds().len();

    let user_count: usize = ctx
        .cache()
        .guilds()
        .iter()
        .filter_map(|guild_id| ctx.cache().guild(*guild_id))
        .map(|guild| guild.member_count as usize)
        .sum();

    let latency_start = std::time::Instant::now();
    let _ = ctx.http().get_current_user().await?;
    let latency_ms = latency_start.elapsed().as_millis() as u64;

    let command_count = ctx.framework().options().commands.len();

    let mut system = System::new();
    system.refresh_memory();
    let memory_mb = system.used_memory() as f64 / 1024.0;

    Ok(AboutStats {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime,
        guild_count,
        user_count,
        latency_ms,
        command_count,
        memory_mb,
    })
}
