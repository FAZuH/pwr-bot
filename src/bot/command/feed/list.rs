//! Feed list subcommand.
use std::time::Duration;

use crate::bot::command::feed::SendInto;
use crate::bot::command::feed::get_or_create_subscriber;
use crate::bot::command::prelude::*;
use crate::bot::gui::Host;
use crate::bot::gui::feed_list::FeedListConfig;
use crate::bot::gui::feed_list::FeedListEffectHandler;
use crate::bot::gui::feed_list::FeedListFeature;

/// Number of items per page for subscriptions list.
pub(crate) const SUBSCRIPTIONS_PER_PAGE: u32 = 10;

/// List your current feed subscriptions
///
/// View all feeds you are subscribed to, with pagination support.
#[poise::command(slash_command)]
pub async fn list(
    ctx: Context<'_>,
    #[description = "Where the notifications are being sent. Default to DM"] sent_into: Option<
        SendInto,
    >,
) -> Result<(), Error> {
    let sent_into = sent_into.unwrap_or(SendInto::DM);
    Router::new(ctx)
        .run(Navigation::FeedList(Some(sent_into)))
        .await?;
    Ok(())
}

handler! { pub struct FeedListHandler<'a> {
    send_into: SendInto
} }

#[async_trait::async_trait]
impl CommandHandler for FeedListHandler<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let subscriber = get_or_create_subscriber(ctx, &self.send_into).await?;

        let service = ctx.data().service.feed_subscription.clone();

        let subscriptions = service
            .list_paginated_subscriptions(&subscriber, 1u32, SUBSCRIPTIONS_PER_PAGE)
            .await?;

        let config = FeedListConfig { subscriptions };
        let handler = FeedListEffectHandler::new(service, subscriber);

        let mut host = Host::<FeedListFeature, _>::new(
            ctx,
            config,
            handler,
            Duration::from_secs(120),
            coordinator.clone(),
        );

        host.run().await?;

        Ok(())
    }
}
