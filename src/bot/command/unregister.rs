//! Admin unregister command.

use crate::bot::command::prelude::*;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::unregister::UnregisterFeature;
use crate::bot::view::ActionRegistry;
use crate::update::unregister::UnregisterModel;
use crate::update::unregister::UnregisterMsg;

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

    let mut model = UnregisterModel::new();
    let msg = ctx.send(build_unregister_reply(&model)).await?;

    guild_id.set_commands(ctx.http(), &[]).await?;

    let duration_ms = start_time.elapsed().as_millis() as u64;
    UnregisterFeature::update(UnregisterMsg::Unregistered { duration_ms }, &mut model);
    msg.edit(ctx, build_unregister_reply(&model)).await?;

    Ok(())
}

/// Builds the unregistration status reply for the given model.
fn build_unregister_reply(model: &UnregisterModel) -> poise::CreateReply<'_> {
    let mut registry = ActionRegistry::new();
    let components = UnregisterFeature::view(model, &mut registry);
    poise::CreateReply::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(components)
}
