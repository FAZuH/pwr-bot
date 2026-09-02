//! Test step for the `/feed list` command.

use crate::bot::command::feed::list::SUBSCRIPTIONS_PER_PAGE;
use crate::bot::command::prelude::*;
use crate::bot::gui::feed_list::FeedListFeature;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::helpers::apply_feature_msg;
use crate::bot::test_framework::helpers::feature_actions;
use crate::bot::test_framework::helpers::translate_feature_action;
use crate::entity::FeedEntity;
use crate::service::feed_subscription::Subscription;
use crate::update::feed_list::FeedListModel;

pub async fn feed_list_empty(_ctx: Context<'_>) -> Result<(), GuiTestError> {
    let feed = FeedEntity {
        id: 1,
        name: "Test Feed".to_string(),
        description: "A test feed".to_string(),
        platform_id: "test".to_string(),
        source_id: "test123".to_string(),
        items_id: "test123".to_string(),
        source_url: "https://example.com/test".to_string(),
        cover_url: "https://example.com/cover.png".to_string(),
        tags: "test".to_string(),
    };

    let subscription = Subscription {
        feed,
        feed_latest: None,
    };

    let mut model = FeedListModel::new(vec![subscription], SUBSCRIPTIONS_PER_PAGE);

    // Initial view mode should have Edit button
    let registry = feature_actions::<FeedListFeature>(&model);
    let edit_action = assert_has_action(&registry, "✎ Edit Subscriptions")
        .map_err(|e| GuiTestError::execution_failed("feed_list render", e))?;

    // Click Edit → edit mode, no effects
    let msg =
        translate_feature_action::<FeedListFeature>(&edit_action, &model).ok_or_else(|| {
            GuiTestError::execution_failed(
                "feed_list edit",
                "action did not translate to a message",
            )
        })?;
    let effects = apply_feature_msg::<FeedListFeature>(msg, &mut model);
    if !effects.is_empty() {
        return Err(GuiTestError::execution_failed(
            "feed_list edit",
            "expected no effects",
        ));
    }

    // Re-render in edit mode should have View Mode button
    let registry = feature_actions::<FeedListFeature>(&model);
    assert_has_action(&registry, "👁 View Mode")
        .map_err(|e| GuiTestError::execution_failed("feed_list edit render", e))?;

    Ok(())
}
