//! Test step for the `/welcome` settings command.

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::bot::command::welcome::SettingsWelcomeHandler;
use crate::bot::command::welcome::image_generator::WelcomeImageGenerator;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_eq_cmd;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::helpers::extract_actions;
use crate::bot::test_framework::helpers::simulate_click;
use crate::bot::view::ViewCommand;
use crate::update::welcome_settings::WelcomeSettingsModel;

pub async fn test_welcome_settings(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let guild_id = ctx.guild_id().ok_or(GuiTestError::assertion_failed(
        "welcome_settings",
        "guild context",
        "none",
    ))?;

    let service = ctx.data().service.feed_subscription.clone();
    let settings = service
        .get_server_settings(guild_id.into())
        .await
        .map_err(|e| GuiTestError::setup_failed("welcome_settings", e))?;

    let generator = Arc::new(WelcomeImageGenerator::new());

    let mut handler = SettingsWelcomeHandler {
        model: WelcomeSettingsModel::new(settings.welcome.clone()),
        settings: settings.clone(),
        current_image_bytes: None,
        service,
        generator,
        guild_id: guild_id.into(),
        ctx_serenity: ctx.serenity_context().clone(),
    };

    let registry = extract_actions(&handler);
    let toggle_action = assert_has_action(&registry, "ToggleEnabled")
        .map_err(|e| GuiTestError::execution_failed("welcome_settings render", e))?;
    assert_has_action(&registry, "❮ Back")
        .map_err(|e| GuiTestError::execution_failed("welcome_settings render", e))?;

    let initial_enabled = handler.model.is_enabled();
    let coordinator = Coordinator::new(ctx);
    let cmd = simulate_click(ctx, &mut handler, toggle_action, coordinator.clone())
        .await
        .map_err(|e| GuiTestError::execution_failed("welcome_settings toggle", e))?;
    assert_eq_cmd(cmd, ViewCommand::Render, "welcome_settings toggle")
        .map_err(|e| GuiTestError::execution_failed("welcome_settings toggle", e))?;
    if handler.model.is_enabled() == initial_enabled {
        return Err(GuiTestError::assertion_failed(
            "welcome_settings toggle",
            !initial_enabled,
            initial_enabled,
        ));
    }

    Ok(())
}
