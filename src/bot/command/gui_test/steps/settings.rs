//! Test steps for settings commands.

use crate::bot::command::feed::settings::SettingsFeedHandler;
use crate::bot::command::prelude::*;
use crate::bot::gui::voice_settings::VoiceSettingsFeature;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_eq_cmd;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::assert::assert_navigated_to;
use crate::bot::test_framework::helpers::apply_feature_msg;
use crate::bot::test_framework::helpers::extract_actions;
use crate::bot::test_framework::helpers::feature_actions;
use crate::bot::test_framework::helpers::simulate_click;
use crate::bot::test_framework::helpers::translate_feature_action;
use crate::bot::view::ViewCmd;
use crate::update::feed_settings::FeedSettingsModel;
use crate::update::voice_settings::VoiceSettingsModel;

pub async fn feed_settings(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let guild_id = ctx.guild_id().ok_or(GuiTestError::assertion_failed(
        "feed_settings",
        "guild context",
        "none",
    ))?;

    let mut settings = ctx
        .data()
        .service
        .feed_subscription
        .get_server_settings(guild_id.into())
        .await
        .map_err(|e| GuiTestError::setup_failed("feed_settings", e))?;

    let feeds_settings = settings.feeds.clone();
    let mut handler = SettingsFeedHandler {
        model: FeedSettingsModel {
            enabled: feeds_settings.enabled,
            channel_id: feeds_settings.channel_id,
            subscribe_role_id: feeds_settings.subscribe_role_id,
            unsubscribe_role_id: feeds_settings.unsubscribe_role_id,
        },
        settings: &mut settings,
    };

    let registry = extract_actions(&handler);
    let toggle_action = assert_has_action(&registry, "Enabled")
        .map_err(|e| GuiTestError::execution_failed("feed_settings render", e))?;
    assert_has_action(&registry, "❮ Back")
        .map_err(|e| GuiTestError::execution_failed("feed_settings render", e))?;

    // Test toggle enabled
    let initial_enabled = handler.model.is_enabled();
    let coordinator = Router::new(ctx);
    let cmd = simulate_click(ctx, &mut handler, toggle_action, coordinator.clone())
        .await
        .map_err(|e| GuiTestError::execution_failed("feed_settings toggle", e))?;
    assert_eq_cmd(cmd, ViewCmd::Render, "feed_settings toggle")
        .map_err(|e| GuiTestError::execution_failed("feed_settings toggle", e))?;
    if handler.model.is_enabled() == initial_enabled {
        return Err(GuiTestError::assertion_failed(
            "feed_settings toggle",
            !initial_enabled,
            initial_enabled,
        ));
    }

    // Test Back navigation
    let coordinator2 = Router::new(ctx);
    let back_action = assert_has_action(&registry, "❮ Back")
        .map_err(|e| GuiTestError::execution_failed("feed_settings", e))?;
    let cmd = simulate_click(ctx, &mut handler, back_action, coordinator2.clone())
        .await
        .map_err(|e| GuiTestError::execution_failed("feed_settings back", e))?;
    assert_eq_cmd(cmd, ViewCmd::Exit, "feed_settings back")
        .map_err(|e| GuiTestError::execution_failed("feed_settings back", e))?;
    assert_navigated_to(&coordinator2, Navigation::SettingsMain)
        .await
        .map_err(|e| GuiTestError::execution_failed("feed_settings nav", e))?;

    Ok(())
}

pub async fn voice_settings(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let guild_id = ctx.guild_id().ok_or(GuiTestError::assertion_failed(
        "voice_settings",
        "guild context",
        "none",
    ))?;

    let service = ctx.data().service.voice_tracking.clone();
    let settings = service
        .get_server_settings(guild_id.into())
        .await
        .map_err(|e| GuiTestError::setup_failed("voice_settings", e))?;

    let mut model = VoiceSettingsModel::new(settings);

    let registry = feature_actions::<VoiceSettingsFeature>(&model);
    let toggle_action = assert_has_action(&registry, "ToggleEnabled")
        .map_err(|e| GuiTestError::execution_failed("voice_settings render", e))?;

    let initial_enabled = model.voice_enabled();
    let msg = translate_feature_action::<VoiceSettingsFeature>(&toggle_action, &model).ok_or_else(
        || {
            GuiTestError::execution_failed(
                "voice_settings toggle",
                "action did not translate to a message",
            )
        },
    )?;
    let effects = apply_feature_msg::<VoiceSettingsFeature>(msg, &mut model);
    if !effects.is_empty() {
        return Err(GuiTestError::execution_failed(
            "voice_settings toggle",
            "expected no effects",
        ));
    }
    if model.voice_enabled() == initial_enabled {
        return Err(GuiTestError::assertion_failed(
            "voice_settings toggle",
            !initial_enabled,
            initial_enabled,
        ));
    }

    Ok(())
}
