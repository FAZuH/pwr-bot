//! About command showing bot statistics and information.
use std::sync::Arc;
use std::time::Duration;

use chrono::Datelike;
use chrono::Utc;
use poise::Command;
use pwr_ext::component;

use crate::bot::command::prelude::*;

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

        let view = AboutView { stats, avatar_url };

        let mut engine = ViewEngine::new(ctx, view, Duration::from_secs(120), coordinator.clone());

        engine.run().await?;

        Ok(())
    }
}

action_enum! {
    AboutAction {
        #[label = "❮ Back"]
        Back,
    }
}

/// View for displaying bot statistics and information.
pub struct AboutView {
    pub(crate) stats: AboutStats,
    pub(crate) avatar_url: String,
}

impl AboutView {
    /// Formats a duration into a human-readable uptime string.
    fn format_uptime(duration: Duration) -> String {
        let days = duration.as_secs() / 86400;
        let hours = (duration.as_secs() % 86400) / 3600;
        let minutes = (duration.as_secs() % 3600) / 60;

        if days > 0 {
            format!("{days} days, {hours} hours, {minutes} minutes")
        } else if hours > 0 {
            format!("{hours} hours, {minutes} minutes")
        } else {
            format!("{minutes} minutes")
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

impl ViewRender for AboutView {
    type Action = AboutAction;
    fn render(&self, registry: &mut ActionRegistry<AboutAction>) -> ResponseKind<'_> {
        let content_text = format!(
            "-# **Settings > About**\n## pwr-bot\n### Stats\n- **Uptime**: {}\n- **Servers**: {}\n- **Users**: {}\n- **Commands**: {}\n- **Latency**: {}ms\n- **Memory**: {:.1} MB\n### Info\n- **Author**: [FAZuH](https://github.com/FAZuH)\n- **Source**: [GitHub](https://github.com/FAZuH/pwr-bot)\n- **License**: [MIT](https://github.com/FAZuH/pwr-bot/blob/main/LICENSE)\nCopyright © 2025-{} FAZuH  —  v{}",
            Self::format_uptime(self.stats.uptime),
            Self::format_number(self.stats.guild_count),
            Self::format_number(self.stats.user_count),
            self.stats.command_count,
            self.stats.latency_ms,
            self.stats.memory_mb,
            self.stats.current_year,
            self.stats.version,
        );

        let back_action = registry.register(AboutAction::Back);

        let container = component! {
            container {
                section {
                    text_display { content: content_text }
                    thumbnail { media: self.avatar_url.clone() }
                }
                action_row {
                    button { url: "https://github.com/FAZuH/pwr-bot", label: "Source Code" }
                    button { url: "https://github.com/FAZuH/pwr-bot/blob/main/LICENSE", label: "License" }
                }
            }
        };

        let back_button = component! {
            action_row {
                button {
                    custom_id: back_action.id,
                    label: back_action.label,
                    style: ButtonStyle::Secondary
                }
            }
        };

        vec![
            CreateComponent::Container(container),
            CreateComponent::ActionRow(back_button),
        ]
        .into()
    }
}

#[async_trait::async_trait]
impl ViewHandler for AboutView {
    type Action = AboutAction;
    async fn handle(&mut self, ctx: ViewContext<'_, AboutAction>) -> Result<ViewCmd, Error> {
        match ctx.action() {
            AboutAction::Back => {
                ctx.coordinator.navigate(Navigation::SettingsMain).await;
                Ok(ViewCmd::Exit)
            }
        }
    }
}

/// Statistics displayed in the about command.
#[derive(Debug, Clone)]
pub struct AboutStats {
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::bot::view::ActionRegistry;
    use crate::bot::view::ResponseKind;

    /// Rewrites every `custom_id` of the shape `Type:timestamp:counter` to a
    /// stable sentinel `id:Type`, so the rendered shape is reproducible across
    /// runs while still pinning kind/label/style/prefix/order.
    fn normalize_custom_ids(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(cid)) = map.get("custom_id") {
                    let parts: Vec<&str> = cid.split(':').collect();
                    if parts.len() == 3
                        && parts[1].chars().all(|c| c.is_ascii_digit())
                        && parts[2].chars().all(|c| c.is_ascii_digit())
                    {
                        let replacement = json!(format!("id:{}", parts[0]));
                        map.insert("custom_id".to_string(), replacement);
                    }
                }
                for v in map.values_mut() {
                    normalize_custom_ids(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    normalize_custom_ids(v);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn about_view_render_snapshot() {
        let stats = AboutStats {
            version: "0.1.0".to_string(),
            uptime: Duration::from_secs(90_000),
            guild_count: 2,
            user_count: 150,
            latency_ms: 42,
            command_count: 12,
            memory_mb: 320.0,
            current_year: 2026,
        };
        let view = AboutView {
            stats,
            avatar_url: "https://example.com/avatar.png".to_string(),
        };

        let mut registry = ActionRegistry::new();
        let response = view.render(&mut registry);
        let ResponseKind::Component(components) = response else {
            panic!("expected a component response");
        };
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);

        let expected = json!([
            {
                "type": 17,
                "components": [
                    {
                        "type": 9,
                        "components": [
                            {
                                "type": 10,
                                "content": "-# **Settings > About**\n## pwr-bot\n### Stats\n- **Uptime**: 1 days, 1 hours, 0 minutes\n- **Servers**: 2\n- **Users**: 150\n- **Commands**: 12\n- **Latency**: 42ms\n- **Memory**: 320.0 MB\n### Info\n- **Author**: [FAZuH](https://github.com/FAZuH)\n- **Source**: [GitHub](https://github.com/FAZuH/pwr-bot)\n- **License**: [MIT](https://github.com/FAZuH/pwr-bot/blob/main/LICENSE)\nCopyright © 2025-2026 FAZuH  —  v0.1.0"
                            }
                        ],
                        "accessory": {
                            "type": 11,
                            "media": { "url": "https://example.com/avatar.png" }
                        }
                    },
                    {
                        "type": 1,
                        "components": [
                            {
                                "type": 2,
                                "disabled": false,
                                "label": "Source Code",
                                "style": 5,
                                "url": "https://github.com/FAZuH/pwr-bot"
                            },
                            {
                                "type": 2,
                                "disabled": false,
                                "label": "License",
                                "style": 5,
                                "url": "https://github.com/FAZuH/pwr-bot/blob/main/LICENSE"
                            }
                        ]
                    }
                ]
            },
            {
                "type": 1,
                "components": [
                    {
                        "type": 2,
                        "custom_id": "id:AboutAction",
                        "disabled": false,
                        "label": "❮ Back",
                        "style": 2
                    }
                ]
            }
        ]);

        assert_eq!(value, expected);
    }
}
