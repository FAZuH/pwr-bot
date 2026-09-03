//! Test step for the `/welcome` settings command.

use crate::bot::command::prelude::*;
use crate::bot::gui::welcome::WelcomeFeature;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::helpers::apply_feature_msg;
use crate::bot::test_framework::helpers::feature_actions;
use crate::bot::test_framework::helpers::translate_feature_action;
use crate::update::welcome_settings::WelcomeSettingsEffect;
use crate::update::welcome_settings::WelcomeSettingsModel;

pub async fn welcome_settings(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let guild_id = ctx.guild_id().ok_or(GuiTestError::assertion_failed(
        "welcome_settings",
        "guild context",
        "none",
    ))?;

    let settings = ctx
        .data()
        .service
        .feed_subscription
        .get_server_settings(guild_id.into())
        .await
        .map_err(|e| GuiTestError::setup_failed("welcome_settings", e))?;

    let mut model = WelcomeSettingsModel::new(settings, None);

    let registry = feature_actions::<WelcomeFeature>(&model);
    let toggle_action = assert_has_action(&registry, "ToggleEnabled")
        .map_err(|e| GuiTestError::execution_failed("welcome_settings render", e))?;
    assert_has_action(&registry, "❮ Back")
        .map_err(|e| GuiTestError::execution_failed("welcome_settings render", e))?;

    let initial_enabled = model.is_enabled();
    let msg =
        translate_feature_action::<WelcomeFeature>(&toggle_action, &model).ok_or_else(|| {
            GuiTestError::execution_failed(
                "welcome_settings toggle",
                "action did not translate to a message",
            )
        })?;
    let effects = apply_feature_msg::<WelcomeFeature>(msg, &mut model);
    if !effects
        .iter()
        .any(|e| matches!(e, WelcomeSettingsEffect::PersistSettings(_)))
    {
        return Err(GuiTestError::execution_failed(
            "welcome_settings toggle",
            "expected a PersistSettings effect",
        ));
    }
    if model.is_enabled() == initial_enabled {
        return Err(GuiTestError::assertion_failed(
            "welcome_settings toggle",
            !initial_enabled,
            initial_enabled,
        ));
    }

    Ok(())
}
