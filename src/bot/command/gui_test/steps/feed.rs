//! Test step for the `/feed list` command.

use crate::bot::command::feed::list::FeedListHandler;
use crate::bot::command::feed::list::SUBSCRIPTIONS_PER_PAGE;
use crate::bot::command::prelude::*;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_eq_cmd;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::helpers::extract_actions;
use crate::bot::test_framework::helpers::simulate_click;
use crate::bot::view::ViewCmd;
use crate::entity::FeedEntity;
use crate::entity::SubscriberEntity;
use crate::entity::SubscriberType;
use crate::service::feed_subscription::Subscription;
use crate::update::feed_list::FeedListModel;

pub async fn test_feed_list_empty(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let subscriber = SubscriberEntity {
        id: 0,
        r#type: SubscriberType::Dm,
        target_id: ctx.author().id.to_string(),
    };

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

    let mut handler = FeedListHandler {
        subscriptions: vec![subscription],
        model: FeedListModel::new(SUBSCRIPTIONS_PER_PAGE),
        service: ctx.data().service.feed_subscription.clone(),
        subscriber,
    };

    // Initial view mode should have Edit button
    let registry = extract_actions(&handler);
    assert_has_action(&registry, "✎ Edit Subscriptions")
        .map_err(|e| GuiTestError::execution_failed("feed_list_empty render", e))?;

    // Click Edit -> should switch to edit mode
    let edit_action = assert_has_action(&registry, "✎ Edit Subscriptions")
        .map_err(|e| GuiTestError::execution_failed("feed_list_empty", e))?;
    let coordinator = Coordinator::new(ctx);
    let cmd = simulate_click(ctx, &mut handler, edit_action, coordinator.clone())
        .await
        .map_err(|e| GuiTestError::execution_failed("feed_list_empty edit", e))?;
    assert_eq_cmd(cmd, ViewCmd::Render, "feed_list_empty edit")
        .map_err(|e| GuiTestError::execution_failed("feed_list_empty edit", e))?;

    // Re-render in edit mode — should have View Mode button
    let registry2 = extract_actions(&handler);
    assert_has_action(&registry2, "👁 View Mode")
        .map_err(|e| GuiTestError::execution_failed("feed_list_empty edit render", e))?;

    Ok(())
}
