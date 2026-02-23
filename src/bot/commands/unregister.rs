//! Admin unregister command.

use poise::serenity_prelude::*;

use crate::bot::checks::is_author_guild_admin;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::error::BotError;
use crate::bot::views::ResponseKind;

/// Unregisters server slash commands
///
/// Removes all bot slash commands from the current server.
/// Requires server administrator permissions.
#[poise::command(prefix_command)]
pub async fn unregister(ctx: Context<'_>) -> Result<(), Error> {
    command(ctx).await
}

pub async fn command(ctx: Context<'_>) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

    let start_time = std::time::Instant::now();

    let mut initial_view = CommandUnregistrationView::new();
    let msg = ctx.send(initial_view.create_reply()).await?;

    guild_id.set_commands(ctx.http(), &[]).await?;

    let duration_ms = start_time.elapsed().as_millis() as u64;
    let mut complete_view = CommandUnregistrationView::new().complete(duration_ms);
    msg.edit(ctx, complete_view.create_reply()).await?;

    Ok(())
}

/// View for command unregistration status.
pub struct CommandUnregistrationView {
    /// Whether unregistration is complete
    is_complete: bool,
    /// Time taken in milliseconds (if complete)
    duration_ms: Option<u64>,
}

impl CommandUnregistrationView {
    /// Creates a new unregistration view.
    pub fn new() -> Self {
        Self {
            is_complete: false,
            duration_ms: None,
        }
    }

    /// Marks the unregistration as complete with duration.
    pub fn complete(mut self, duration_ms: u64) -> Self {
        self.is_complete = true;
        self.duration_ms = Some(duration_ms);
        self
    }

    pub fn create_response(&mut self) -> ResponseKind<'_> {
        let title = if self.is_complete {
            "Command Unregistration Complete"
        } else {
            "Unregistering Commands"
        };

        let status_text = if self.is_complete {
            format!(
                "### {}\nSuccessfully unregistered all commands in {}ms",
                title,
                self.duration_ms.unwrap_or(0)
            )
        } else {
            format!("### {}\nUnregistering all server commands...", title)
        };

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
        ]));

        vec![container].into()
    }

    pub fn create_reply(&mut self) -> poise::CreateReply<'_> {
        self.create_response().into()
    }
}
