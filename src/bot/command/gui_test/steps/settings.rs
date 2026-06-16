//! Test steps for settings commands.

use std::collections::HashMap;

use crate::bot::command::prelude::*;
use crate::bot::command::settings::SettingsMainAction;
use crate::bot::command::settings::SettingsMainView;
use crate::bot::command::settings::collect_features;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_eq_cmd;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::helpers::extract_actions;
use crate::bot::test_framework::helpers::simulate_select;
use crate::bot::view::SelectValues;
use crate::bot::view::ViewCmd;
use crate::entity::Json;
use crate::entity::ServerSettingsEntity;
use crate::update::settings_main::SettingsMainModel;

pub async fn settings_main(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let guild_id = ctx.guild_id().ok_or(GuiTestError::assertion_failed(
        "settings_main",
        "guild context",
        "none",
    ))?;

    let settings = ctx
        .data()
        .service
        .settings
        .get_server_settings(guild_id.into())
        .await
        .map_err(|e| GuiTestError::setup_failed("settings_main", e))?;

    let features = collect_features(&ctx.data().plugin_registry)
        .await
        .into_iter()
        .map(|f| crate::bot::command::settings::Feature {
            id: f.id.clone(),
            label: f.label.clone(),
            navigate: f.navigate.clone(),
        })
        .collect::<Vec<_>>();

    let entity = ServerSettingsEntity {
        guild_id: guild_id.get().into(),
        settings: Json(settings),
    };

    let model = SettingsMainModel::new(
        features
            .iter()
            .map(|f| (f.id.clone(), f.is_enabled(&entity.settings.0)))
            .collect::<HashMap<_, _>>(),
    );

    let first_feature = features.first().cloned().ok_or_else(|| {
        GuiTestError::setup_failed("settings_main", "no features available to toggle")
    })?;

    let mut view = SettingsMainView {
        settings: entity,
        model,
        features,
    };

    let registry = extract_actions(&view);
    assert_has_action(&registry, "NavigateToFeature")
        .map_err(|e| GuiTestError::execution_failed("settings_main render nav", e))?;
    assert_has_action(&registry, "🛈 About")
        .map_err(|e| GuiTestError::execution_failed("settings_main render about", e))?;

    // Test toggle with the first available feature
    let coordinator = Router::new(ctx);
    let toggle_action = registry
        .actions
        .values()
        .find(|a| matches!(a, SettingsMainAction::ToggleFeature))
        .cloned()
        .unwrap();
    let initial_enabled = view.model.is_enabled(&first_feature.id);
    let cmd = simulate_select(
        ctx,
        &mut view,
        toggle_action,
        SelectValues::String(vec![first_feature.id.clone()]),
        coordinator.clone(),
    )
    .await
    .map_err(|e| GuiTestError::execution_failed("settings_main toggle", e))?;
    assert_eq_cmd(cmd, ViewCmd::Render, "settings_main toggle")
        .map_err(|e| GuiTestError::execution_failed("settings_main toggle", e))?;
    if view.model.is_enabled(&first_feature.id) == initial_enabled {
        return Err(GuiTestError::assertion_failed(
            "settings_main toggle",
            !initial_enabled,
            initial_enabled,
        ));
    }

    Ok(())
}
