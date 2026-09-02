//! Test steps for settings commands.

use crate::bot::command::prelude::*;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feed_settings::FeedSettingsFeature;
use crate::bot::gui::voice_settings::VoiceSettingsFeature;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::helpers::apply_feature_msg;
use crate::bot::test_framework::helpers::feature_actions;
use crate::bot::test_framework::helpers::translate_feature_action;
use crate::update::feed_settings::FeedSettingsEffect;
use crate::update::feed_settings::FeedSettingsModel;
use crate::update::feed_settings::FeedSettingsMsg;
use crate::update::voice_settings::VoiceSettingsModel;

pub async fn feed_settings(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let guild_id = ctx.guild_id().ok_or(GuiTestError::assertion_failed(
        "feed_settings",
        "guild context",
        "none",
    ))?;

    let settings = ctx
        .data()
        .service
        .feed_subscription
        .get_server_settings(guild_id.into())
        .await
        .map_err(|e| GuiTestError::setup_failed("feed_settings", e))?;

    let mut model = FeedSettingsModel::new(settings);

    let registry = feature_actions::<FeedSettingsFeature>(&model);
    let toggle_action = assert_has_action(&registry, "Enabled")
        .map_err(|e| GuiTestError::execution_failed("feed_settings render", e))?;

    // Test toggle enabled
    let initial_enabled = model.is_enabled();
    let msg = translate_feature_action::<FeedSettingsFeature>(&toggle_action, &model).ok_or_else(
        || {
            GuiTestError::execution_failed(
                "feed_settings toggle",
                "action did not translate to a message",
            )
        },
    )?;
    let effects = apply_feature_msg::<FeedSettingsFeature>(msg, &mut model);
    if !effects.is_empty() {
        return Err(GuiTestError::execution_failed(
            "feed_settings toggle",
            "expected no effects",
        ));
    }
    if model.is_enabled() == initial_enabled {
        return Err(GuiTestError::assertion_failed(
            "feed_settings toggle",
            !initial_enabled,
            initial_enabled,
        ));
    }

    // Test Back navigation (also triggers persistence)
    let back = assert_has_action(&registry, "❮ Back")
        .map_err(|e| GuiTestError::execution_failed("feed_settings", e))?;
    let msg = translate_feature_action::<FeedSettingsFeature>(&back, &model).ok_or_else(|| {
        GuiTestError::execution_failed(
            "feed_settings back",
            "action did not translate to a message",
        )
    })?;
    let effects = apply_feature_msg::<FeedSettingsFeature>(msg, &mut model);
    if !effects
        .iter()
        .any(|e| matches!(e, FeedSettingsEffect::PersistSettings(_)))
    {
        return Err(GuiTestError::execution_failed(
            "feed_settings back",
            "expected a PersistSettings effect",
        ));
    }
    if FeedSettingsFeature::exit_navigation(&FeedSettingsMsg::Back)
        != Some(Navigation::SettingsMain)
    {
        return Err(GuiTestError::assertion_failed(
            "feed_settings back",
            format!("{:?}", Some(Navigation::SettingsMain)),
            format!(
                "{:?}",
                FeedSettingsFeature::exit_navigation(&FeedSettingsMsg::Back)
            ),
        ));
    }

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
