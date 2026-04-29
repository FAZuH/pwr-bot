//! Test steps for settings commands.

use crate::bot::command::feed::settings::SettingsFeedHandler;
use crate::bot::command::prelude::*;
use crate::bot::command::settings::SettingsMainAction;
use crate::bot::command::settings::SettingsMainHandler;
use crate::bot::command::voice::settings::SettingsVoiceHandler;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_eq_cmd;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::assert::assert_navigated_to;
use crate::bot::test_framework::helpers::extract_actions;
use crate::bot::test_framework::helpers::simulate_click;
use crate::bot::test_framework::helpers::simulate_select;
use crate::bot::view::SelectValues;
use crate::bot::view::ViewCommand;
use crate::entity::ServerSettingsEntity;
use crate::update::feed_settings::FeedSettingsModel;
use crate::update::settings_main::SettingsMainModel;

pub async fn test_settings_main(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let guild_id = ctx.guild_id().ok_or(GuiTestError::assertion_failed(
        "settings_main",
        "guild context",
        "none",
    ))?;

    let settings = ctx
        .data()
        .service
        .feed_subscription
        .get_server_settings(guild_id.into())
        .await
        .map_err(|e| GuiTestError::setup_failed("settings_main", e))?;

    let entity = ServerSettingsEntity {
        guild_id: guild_id.into(),
        settings: sqlx::types::Json(settings),
    };

    let model = SettingsMainModel::new(
        entity.settings.0.feeds.enabled.unwrap_or(false),
        entity.settings.0.voice.enabled.unwrap_or(false),
        entity.settings.0.welcome.enabled.unwrap_or(false),
    );

    let mut handler = SettingsMainHandler {
        settings: entity,
        model,
    };

    let registry = extract_actions(&handler);
    for label in ["Feeds", "Voice", "Welcome", "🛈 About"] {
        assert_has_action(&registry, label)
            .map_err(|e| GuiTestError::execution_failed("settings_main render", e))?;
    }

    // Test Feeds navigation
    let coordinator = Coordinator::new(ctx);
    let feeds_action = registry
        .actions
        .values()
        .find(|a| a.label() == "Feeds")
        .cloned()
        .unwrap();
    let cmd = simulate_click(ctx, &mut handler, feeds_action, coordinator.clone())
        .await
        .map_err(|e| GuiTestError::execution_failed("settings_main feeds", e))?;
    assert_eq_cmd(cmd, ViewCommand::Exit, "settings_main feeds click")
        .map_err(|e| GuiTestError::execution_failed("settings_main feeds", e))?;
    assert_navigated_to(&coordinator, Navigation::SettingsFeeds)
        .map_err(|e| GuiTestError::execution_failed("settings_main nav", e))?;

    // Test toggle
    let coordinator2 = Coordinator::new(ctx);
    let toggle_action = registry
        .actions
        .values()
        .find(|a| matches!(a, SettingsMainAction::ToggleFeature))
        .cloned()
        .unwrap();
    let initial_feeds = handler.model.feeds_enabled;
    let cmd = simulate_select(
        ctx,
        &mut handler,
        toggle_action,
        SelectValues::String(vec!["Feeds".to_string()]),
        coordinator2.clone(),
    )
    .await
    .map_err(|e| GuiTestError::execution_failed("settings_main toggle", e))?;
    assert_eq_cmd(cmd, ViewCommand::Render, "settings_main toggle")
        .map_err(|e| GuiTestError::execution_failed("settings_main toggle", e))?;
    if handler.model.feeds_enabled == initial_feeds {
        return Err(GuiTestError::assertion_failed(
            "settings_main toggle",
            !initial_feeds,
            initial_feeds,
        ));
    }

    Ok(())
}

pub async fn test_feed_settings(ctx: Context<'_>) -> Result<(), GuiTestError> {
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
    let coordinator = Coordinator::new(ctx);
    let cmd = simulate_click(ctx, &mut handler, toggle_action, coordinator.clone())
        .await
        .map_err(|e| GuiTestError::execution_failed("feed_settings toggle", e))?;
    assert_eq_cmd(cmd, ViewCommand::Render, "feed_settings toggle")
        .map_err(|e| GuiTestError::execution_failed("feed_settings toggle", e))?;
    if handler.model.is_enabled() == initial_enabled {
        return Err(GuiTestError::assertion_failed(
            "feed_settings toggle",
            !initial_enabled,
            initial_enabled,
        ));
    }

    // Test Back navigation
    let coordinator2 = Coordinator::new(ctx);
    let back_action = assert_has_action(&registry, "❮ Back")
        .map_err(|e| GuiTestError::execution_failed("feed_settings", e))?;
    let cmd = simulate_click(ctx, &mut handler, back_action, coordinator2.clone())
        .await
        .map_err(|e| GuiTestError::execution_failed("feed_settings back", e))?;
    assert_eq_cmd(cmd, ViewCommand::Exit, "feed_settings back")
        .map_err(|e| GuiTestError::execution_failed("feed_settings back", e))?;
    assert_navigated_to(&coordinator2, Navigation::SettingsMain)
        .map_err(|e| GuiTestError::execution_failed("feed_settings nav", e))?;

    Ok(())
}

pub async fn test_voice_settings(ctx: Context<'_>) -> Result<(), GuiTestError> {
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

    let mut handler = SettingsVoiceHandler { settings };

    let registry = extract_actions(&handler);
    let toggle_action = assert_has_action(&registry, "ToggleEnabled")
        .map_err(|e| GuiTestError::execution_failed("voice_settings render", e))?;

    let initial_enabled = handler.settings.voice.enabled.unwrap_or(true);
    let coordinator = Coordinator::new(ctx);
    let cmd = simulate_click(ctx, &mut handler, toggle_action, coordinator.clone())
        .await
        .map_err(|e| GuiTestError::execution_failed("voice_settings toggle", e))?;
    assert_eq_cmd(cmd, ViewCommand::Render, "voice_settings toggle")
        .map_err(|e| GuiTestError::execution_failed("voice_settings toggle", e))?;
    if handler.settings.voice.enabled.unwrap_or(true) == initial_enabled {
        return Err(GuiTestError::assertion_failed(
            "voice_settings toggle",
            !initial_enabled,
            initial_enabled,
        ));
    }

    Ok(())
}
