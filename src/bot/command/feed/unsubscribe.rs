//! Feed unsubscribe subcommand.

use crate::bot::command::feed::SendInto;
use crate::bot::command::feed::get_or_create_subscriber;
use crate::bot::command::feed::process_subscription_batch;
use crate::bot::command::feed::verify_server_config;
use crate::bot::command::prelude::*;

/// Unsubscribe from one or more feeds
///
/// Remove feeds from your subscriptions. Use autocomplete to find
/// feeds you are currently subscribed to.
// TODO: route unsubscribe DB operations through feed plugin
//   Currently `service.feed_subscription.unsubscribe()` returns a stub error
//   because the actual logic was moved to pwr-bot-plugin-feed.
//   Fix: dispatch via `plugin.invoke("feed.unsubscribe", args)` or extend PluginHost
//   with a `call_procedure` path for synchronous DB operations from the batch UI.
#[poise::command(slash_command)]
pub async fn unsubscribe(
    ctx: Context<'_>,
    #[description = "Link(s) of the feeds. Separate links with commas (,)"]
    #[autocomplete = "autocomplete_subscriptions"]
    links: String,
    #[description = "Where notifications were being sent. Default to DM"] send_into: Option<
        SendInto,
    >,
) -> Result<(), Error> {
    Router::new(ctx)
        .run(Navigation::FeedUnsubscribe { links, send_into })
        .await?;
    Ok(())
}

handler! { pub struct FeedUnsubscribeHandler {
    links: String,
    send_into: Option<SendInto>,
} }

#[async_trait::async_trait]
impl CommandHandler for FeedUnsubscribeHandler {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        self.host_ctx.defer().await?;

        let send_into = self.send_into.unwrap_or(SendInto::DM);
        let urls = parse_and_validate_urls(&self.links)?;

        verify_server_config(ctx, &send_into, false).await?;

        let subscriber = get_or_create_subscriber(ctx, &send_into).await?;
        Ok(process_subscription_batch(coordinator, &urls, &subscriber, false).await?)
    }
}

/// Autocompletes subscriptions for the current user.
///
/// Subscription data comes from the feed plugin. The plugin handles
/// its own autocomplete logic through its command handler.
pub async fn autocomplete_subscriptions<'a>(
    _ctx: Context<'_>,
    _partial: &str,
) -> CreateAutocompleteResponse<'a> {
    CreateAutocompleteResponse::new()
}
