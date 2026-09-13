//! Admin register command.

use poise::samples::create_application_commands;

use crate::bot::command::prelude::*;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::register::RegisterFeature;
use crate::bot::view::ActionRegistry;
use crate::update::register::RegisterModel;
use crate::update::register::RegisterMsg;

/// Registers server slash commands
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

    let mut model = RegisterModel::new(num_commands);
    let msg = ctx.send(build_register_reply(&model)).await?;

    guild_id.set_commands(ctx.http(), &create_commands).await?;

    let duration_ms = start_time.elapsed().as_millis() as u64;
    RegisterFeature::update(RegisterMsg::Registered { duration_ms }, &mut model);
    msg.edit(ctx, build_register_reply(&model)).await?;

    Ok(())
}

/// Builds the registration status reply for the given model.
fn build_register_reply(model: &RegisterModel) -> poise::CreateReply<'_> {
    let mut registry = ActionRegistry::new();
    let components = RegisterFeature::view(model, &mut registry);
    poise::CreateReply::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(components)
}
