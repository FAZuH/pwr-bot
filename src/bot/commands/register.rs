//! Admin register command.

use poise::samples::create_application_commands;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateTextDisplay;

use crate::bot::checks::is_author_guild_admin;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::error::BotError;
use crate::bot::views::ResponseKind;

/// Registers guild slash commands
///
/// Registers all bot slash commands to the current server.
/// Requires server administrator permissions.
#[poise::command(prefix_command)]
pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
    command(ctx).await
}

pub async fn command(ctx: Context<'_>) -> Result<(), Error> {
    is_author_guild_admin(ctx).await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;

    let create_commands = create_application_commands(&ctx.framework().options().commands);
    let num_commands = create_commands.len();

    let start_time = std::time::Instant::now();

    let mut initial_view = CommandRegistrationView::new(num_commands);
    let msg = ctx.send(initial_view.create_reply()).await?;

    guild_id.set_commands(ctx.http(), &create_commands).await?;

    let duration_ms = start_time.elapsed().as_millis() as u64;
    let mut complete_view = CommandRegistrationView::new(num_commands).complete(duration_ms);
    msg.edit(ctx, complete_view.create_reply()).await?;

    Ok(())
}

/// View for command registration status.
pub struct CommandRegistrationView {
    /// Number of commands being registered
    num_commands: usize,
    /// Whether registration is complete
    is_complete: bool,
    /// Time taken in milliseconds (if complete)
    duration_ms: Option<u64>,
}

impl CommandRegistrationView {
    /// Creates a new registration view.
    pub fn new(num_commands: usize) -> Self {
        Self {
            num_commands,
            is_complete: false,
            duration_ms: None,
        }
    }

    /// Marks the registration as complete with duration.
    pub fn complete(mut self, duration_ms: u64) -> Self {
        self.is_complete = true;
        self.duration_ms = Some(duration_ms);
        self
    }

    pub fn create_response(&mut self) -> ResponseKind<'_> {
        let title = if self.is_complete {
            "Command Registration Complete"
        } else {
            "Registering Commands"
        };

        let status_text = if self.is_complete {
            format!(
                "### {}\nSuccessfully registered {} commands in {}ms",
                title,
                self.num_commands,
                self.duration_ms.unwrap_or(0)
            )
        } else {
            format!(
                "### {}\nRegistering {} guild commands...",
                title, self.num_commands
            )
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
