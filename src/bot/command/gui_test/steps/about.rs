//! Test step for the `/about` command.

use crate::bot::command::about::AboutStats;
use crate::bot::command::about::AboutView;
use crate::bot::command::prelude::*;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_eq_cmd;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::assert::assert_navigated_to;
use crate::bot::test_framework::helpers::extract_actions;
use crate::bot::test_framework::helpers::simulate_click;

pub async fn test_about(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let stats = AboutStats::gather_stats(&ctx)
        .await
        .map_err(|e| GuiTestError::setup_failed("about", e))?;
    let avatar_url = ctx.cache().current_user().face();

    let mut view = AboutView { stats, avatar_url };

    let registry = extract_actions(&view);
    assert_has_action(&registry, "❮ Back")
        .map_err(|e| GuiTestError::execution_failed("about render", e))?;

    let coordinator = Coordinator::new(ctx);
    let back_action = assert_has_action(&registry, "❮ Back")
        .map_err(|e| GuiTestError::execution_failed("about", e))?;
    let cmd = simulate_click(ctx, &mut view, back_action, coordinator.clone())
        .await
        .map_err(|e| GuiTestError::execution_failed("about back", e))?;

    assert_eq_cmd(cmd, ViewCommand::Exit, "about back")
        .map_err(|e| GuiTestError::execution_failed("about back", e))?;
    assert_navigated_to(&coordinator, Navigation::SettingsMain)
        .map_err(|e| GuiTestError::execution_failed("about nav", e))?;

    Ok(())
}
