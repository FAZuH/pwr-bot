//! Permission checks for bot commands.

use std::borrow::Cow;

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

    let is_admin = permissions.contains(Permissions::ADMINISTRATOR)
        || permissions.contains(Permissions::MANAGE_GUILD);
    if !is_admin {
        Err(BotError::PermissionDenied(
            "You need `Manage Server` or `Administrator` permission to perform this action."
                .to_string(),
        ))?
    };
    Ok(())
}

/// Whether the command author is the bot owner: a query form of the owner
/// check for commands that change their reply instead of erroring.
pub fn author_is_bot_owner(ctx: Context<'_>) -> bool {
    let author = ctx.author().id;
    let owners = &ctx.framework().options().owners;
    owners.contains(&author)
}

/// Checks if the command author has any of the required roles.
pub async fn check_author_roles<'a>(
    ctx: Context<'_>,
    required_role_ids: impl Into<Cow<'a, [RoleId]>>,
) -> Result<(), Error> {
    let member = ctx
        .author_member()
        .await
        .ok_or(BotError::GuildOnlyCommand)?;
    Ok(check_permissions_inner(
        &member.roles,
        &required_role_ids.into(),
    )?)
}

/// Internal function to check if user has required permissions.
fn check_permissions_inner(
    user_roles: &[RoleId],
    required_role_ids: &[RoleId],
) -> Result<(), BotError> {
    if let Some(id) = required_role_ids.iter().find(|id| !user_roles.contains(id)) {
        return Err(BotError::PermissionDenied(format!(
            "You need the <@&{id}> role to perform this action."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_permissions_with_required_role() {
        let role_id = RoleId::new(123);
        let user_roles = vec![role_id];
        let result = check_permissions_inner(&user_roles, &[RoleId::new(123)]);
        assert!(result.is_ok());
    }

    #[test]
    fn check_permissions_without_required_role_fails() {
        let user_roles = vec![RoleId::new(456)];
        let result = check_permissions_inner(&user_roles, &[RoleId::new(123)]);
        assert!(result.is_err());
    }
}
