//! Permission checks for bot commands.

use poise::serenity_prelude::*;

use crate::bot::command::Context;
use crate::bot::command::Error;
use crate::bot::error::BotError;

/// Checks if the command author has server administrator permissions.
pub async fn is_author_guild_admin(ctx: Context<'_>) -> Result<(), Error> {
    let member = ctx
        .author_member()
        .await
        .ok_or(BotError::GuildOnlyCommand)?;
    let permissions = ctx
        .guild()
        .ok_or(BotError::GuildOnlyCommand)?
        .member_permissions(member.as_ref());

    if !is_guild_admin_permissions(permissions) {
        Err(BotError::PermissionDenied(
            "You need `Manage Server` or `Administrator` permission to perform this action."
                .to_string(),
        ))?
    };
    Ok(())
}

fn is_guild_admin_permissions(permissions: Permissions) -> bool {
    permissions.contains(Permissions::ADMINISTRATOR)
        || permissions.contains(Permissions::MANAGE_GUILD)
}

/// Whether the command author is the bot owner: a query form of the owner
/// check for commands that change their reply instead of erroring.
pub fn author_is_bot_owner(ctx: Context<'_>) -> bool {
    let author = ctx.author().id;
    let owners = &ctx.framework().options().owners;
    owners.contains(&author)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guild_admin_permissions_accept_both_management_bits() {
        assert!(is_guild_admin_permissions(Permissions::ADMINISTRATOR));
        assert!(is_guild_admin_permissions(Permissions::MANAGE_GUILD));
    }

    #[test]
    fn guild_admin_permissions_reject_an_unprivileged_member() {
        assert!(!is_guild_admin_permissions(Permissions::MANAGE_MESSAGES));
    }
}
