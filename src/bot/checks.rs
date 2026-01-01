use std::str::FromStr;

use serenity::all::Permissions;
use serenity::all::RoleId;

use crate::bot::cog::Context;
use crate::bot::cog::Error;
use crate::bot::error::BotError;

pub async fn check_guild_permissions(
    ctx: Context<'_>,
    required_role_id: &Option<String>,
) -> Result<(), Error> {
    let member = ctx
        .author_member()
        .await
        .ok_or(BotError::GuildOnlyCommand)?;
    let permissions = ctx
        .guild()
        .ok_or(BotError::GuildOnlyCommand)?
        .member_permissions(member.as_ref());

    Ok(check_permissions_inner(
        permissions.contains(Permissions::ADMINISTRATOR)
            || permissions.contains(Permissions::MANAGE_GUILD),
        &member.roles,
        required_role_id,
    )?)
}

fn check_permissions_inner(
    is_admin: bool,
    user_roles: &[RoleId],
    required_role_id: &Option<String>,
) -> Result<(), BotError> {
    if is_admin {
        return Ok(());
    }

    if let Some(role_id_str) = required_role_id {
        if let Ok(role_id) = RoleId::from_str(role_id_str)
            && user_roles.contains(&role_id)
        {
            return Ok(());
        }

        return Err(BotError::PermissionDenied(format!(
            "You need the <@&{}> role to perform this action.",
            role_id_str
        )));
    }

    Err(BotError::PermissionDenied(
        "You need the `Manage Server` or `Administrator` permission or a configured role to perform this action."
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::error::BotError;

    #[test]
    fn test_check_permissions_admin_always_passes() {
        let result = check_permissions_inner(true, &[], &None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_permissions_with_required_role() {
        let role_id = RoleId::new(123);
        let user_roles = vec![role_id];
        let result = check_permissions_inner(false, &user_roles, &Some("123".to_string()));
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_permissions_without_required_role_fails() {
        let user_roles = vec![RoleId::new(456)];
        let result = check_permissions_inner(false, &user_roles, &Some("123".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_check_permissions_fails_without_any_permissions() {
        let result = check_permissions_inner(false, &[], &None);
        assert!(result.is_err());
        match result.unwrap_err() {
            BotError::PermissionDenied(msg) => {
                assert!(msg.contains("Manage Server"));
            }
            _ => panic!("Expected PermissionDenied error"),
        }
    }
}
