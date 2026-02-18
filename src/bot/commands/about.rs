//! About command showing bot statistics and information.

use std::time::Duration;

use chrono::Datelike;
use chrono::Utc;
use poise::Command;
use poise::serenity_prelude::ButtonStyle;
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
use serenity::all::ComponentInteraction;

use crate::action_enum;
use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::settings::SettingsPage;
use crate::bot::commands::settings::run_settings;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::InteractiveView;
use crate::bot::views::RenderExt;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::controller;
use crate::view_core;

/// Cog for the about command.
pub struct AboutCog;

impl AboutCog {
    /// Show information about the bot
    #[poise::command(slash_command)]
    pub async fn about(ctx: Context<'_>) -> Result<(), Error> {
        run_settings(ctx, Some(SettingsPage::About)).await
    }
}

impl Cog for AboutCog {
    fn commands(&self) -> Vec<Command<crate::bot::Data, Error>> {
        vec![Self::about()]
    }
}

controller! { pub struct AboutController<'a> {} }

#[async_trait::async_trait]
impl<S: Send + Sync + 'static> Controller<S> for AboutController<'_> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let stats = AboutStats::gather_stats(&ctx).await?;
        let avatar_url = ctx.cache().current_user().face();
        let mut view = AboutView::new(&ctx, stats, avatar_url);

        view.render().await?;

        // Wait for user interaction (Back button)
        if let Some((action, _)) = view.listen_once().await? {
            match action {
                AboutAction::Back => {
                    return Ok(NavigationResult::Back);
                }
            }
        }

        // If interaction times out, just exit
        Ok(NavigationResult::Exit)
    }
}

action_enum! {
    AboutAction {
        #[label = "< Back"]
        Back,
    }
}

view_core! {
    timeout = Duration::from_secs(120),
    /// View for displaying bot statistics and information.
    struct AboutView<'a, AboutAction> {
        stats: AboutStats,
        avatar_url: String,
    }
}

impl<'a> AboutView<'a> {
    /// Creates a new about view with the given context, stats, and avatar URL.
    pub fn new(ctx: &'a Context<'a>, stats: AboutStats, avatar_url: String) -> Self {
        Self {
            core: Self::create_core(ctx),
            stats,
            avatar_url,
        }
    }

    /// Formats a duration into a human-readable uptime string.
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

    /// Formats a number with k/M suffixes for readability.
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

impl<'a> ResponseView<'a> for AboutView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let content_text = format!(
            "-# **Settings > About**
## pwr-bot
### Stats
- **Uptime**: {}
- **Servers**: {}
- **Users**: {}
- **Commands**: {}
- **Latency**: {}ms
- **Memory**: {:.1} MB
### Info
- **Author**: [FAZuH](https://github.com/FAZuH)
- **Source**: [GitHub](https://github.com/FAZuH/pwr-bot)
- **License**: [MIT](https://github.com/FAZuH/pwr-bot/blob/main/LICENSE)
Copyright © 2025-{} FAZuH  —  v{}",
            Self::format_uptime(self.stats.uptime),
            Self::format_number(self.stats.guild_count),
            Self::format_number(self.stats.user_count),
            self.stats.command_count,
            self.stats.latency_ms,
            self.stats.memory_mb,
            self.stats.current_year,
            self.stats.version,
        );

        let avatar_url: String = self.avatar_url.clone();
        let avatar = CreateThumbnail::new(CreateUnfurledMediaItem::new(avatar_url));

        let content_section = CreateSection::new(
            vec![CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                content_text,
            ))],
            CreateSectionAccessory::Thumbnail(avatar),
        );

        let github_button =
            CreateButton::new_link("https://github.com/FAZuH/pwr-bot").label("Source Code");

        let license_button =
            CreateButton::new_link("https://github.com/FAZuH/pwr-bot/blob/main/LICENSE")
                .label("License");

        // Register the back action and get its ID
        let back_button = CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                self.register(AboutAction::Back)
                    .as_button()
                    .style(ButtonStyle::Secondary),
            ]
            .into(),
        ));

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::Section(content_section),
            CreateContainerComponent::ActionRow(CreateActionRow::Buttons(
                vec![github_button, license_button].into(),
            )),
        ]));

        vec![container, back_button].into()
    }
}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, AboutAction> for AboutView<'a> {
    async fn handle(
        &mut self,
        action: &AboutAction,
        _interaction: &ComponentInteraction,
    ) -> Option<AboutAction> {
        match action {
            AboutAction::Back => Some(AboutAction::Back),
        }
    }
}

/// Statistics displayed in the about command.
struct AboutStats {
    version: String,
    uptime: Duration,
    guild_count: usize,
    user_count: usize,
    latency_ms: u64,
    command_count: usize,
    memory_mb: f64,
    current_year: i32,
}

impl AboutStats {
    /// Gathers bot statistics for the about command.
    async fn gather_stats(ctx: &Context<'_>) -> Result<AboutStats, Error> {
        let start_time = ctx.data().start_time;
        let version = ctx.data().config.version.clone();
        let uptime = start_time.elapsed();

        let guild_count = ctx.cache().guilds().len();

        let user_count: usize = ctx
            .cache()
            .guilds()
            .iter()
            .filter_map(|guild_id| {
                ctx.cache()
                    .guild(*guild_id)
                    .map(|guild| guild.member_count as usize)
            })
            .sum();

        // Make a request to Discord server to get latency
        let latency_start = std::time::Instant::now();
        let _ = ctx.http().get_current_user().await?;
        let latency_ms = latency_start.elapsed().as_millis() as u64;

        let command_count = Self::count_commands(&ctx.framework().options().commands);

        let memory_mb = Self::get_process_memory_mb();

        let current_year = Utc::now().year();

        Ok(AboutStats {
            version,
            uptime,
            guild_count,
            user_count,
            latency_ms,
            command_count,
            memory_mb,
            current_year,
        })
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
