//! Test step for the `/about` command.
//!
//! Drives the [`AboutFeature`] through the msg-driven [`GuiFeature`] API:
//! render → assert the back action, translate a back click into a `Msg`, feed
//! it into the pure `update`, and assert the navigation target.

use crate::bot::command::prelude::*;
use crate::bot::gui::about::AboutFeature;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::helpers::apply_feature_msg;
use crate::bot::test_framework::helpers::feature_actions;
use crate::bot::test_framework::helpers::translate_feature_action;
use crate::update::about::AboutModel;
use crate::update::about::AboutMsg;
use crate::update::about::AboutStats;

pub async fn about(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let stats = AboutStats::gather_stats(&ctx)
        .await
        .map_err(|e| GuiTestError::setup_failed("about", e))?;
    let avatar_url = ctx.cache().current_user().face();
    let mut model = AboutModel::new(stats, avatar_url);

    let registry = feature_actions::<AboutFeature>(&model);
    assert_has_action(&registry, "❮ Back")
        .map_err(|e| GuiTestError::execution_failed("about render", e))?;

    let back = assert_has_action(&registry, "❮ Back")
        .map_err(|e| GuiTestError::execution_failed("about", e))?;
    let msg = translate_feature_action::<AboutFeature>(&back, &model).ok_or_else(|| {
        GuiTestError::execution_failed("about back", "action did not translate to a message")
    })?;

    let effects = apply_feature_msg::<AboutFeature>(msg, &mut model);
    if !effects.is_empty() {
        return Err(GuiTestError::execution_failed(
            "about back",
            "expected no effects",
        ));
    }

    if AboutFeature::exit_navigation(&AboutMsg::Back) != Some(Navigation::SettingsMain) {
        return Err(GuiTestError::assertion_failed(
            "about back",
            format!("{:?}", Some(Navigation::SettingsMain)),
            format!("{:?}", AboutFeature::exit_navigation(&AboutMsg::Back)),
        ));
    }

    Ok(())
}
