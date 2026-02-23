//! Feed unsubscribe subcommand.

use poise::serenity_prelude::*;

use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::feed::SendInto;
use crate::bot::commands::feed::get_or_create_subscriber;
use crate::bot::commands::feed::process_subscription_batch;
use crate::bot::commands::feed::verify_server_config;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::navigation::NavigationResult;
use crate::bot::utils::parse_and_validate_urls;
use crate::controller;

/// Unsubscribe from one or more feeds
///
/// Remove feeds from your subscriptions. Use autocomplete to find
/// feeds you are currently subscribed to.
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
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = FeedUnsubscribeController::new(&ctx, links, send_into);
    let _result = controller.run(&mut coordinator).await?;
    Ok(())
}

controller! { pub struct FeedUnsubscribeController<'a> {
    links: String,
    send_into: Option<SendInto>,
} }

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for FeedUnsubscribeController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let send_into = self.send_into.unwrap_or(SendInto::DM);
        let urls = parse_and_validate_urls(&self.links)?;

        verify_server_config(ctx, &send_into, false).await?;

        let subscriber = get_or_create_subscriber(ctx, &send_into).await?;
        Ok(process_subscription_batch(ctx, &urls, &subscriber, false).await?)
    }
}

/// Autocompletes subscriptions for the current user.
pub async fn autocomplete_subscriptions<'a>(
    ctx: Context<'_>,
    partial: &str,
) -> CreateAutocompleteResponse<'a> {
    if partial.trim().is_empty() {
        return CreateAutocompleteResponse::new().set_choices(vec![AutocompleteChoice::from(
            "Start typing to see suggestions",
        )]);
    }

    let service = ctx.data().service.feed_subscription.clone();

    let (user_sub, guild_sub) = service
        .get_both_subscribers(
            ctx.author().id.to_string(),
            ctx.guild_id().map(|v| v.to_string()),
        )
        .await;

    if user_sub.is_none() && guild_sub.is_none() {
        return CreateAutocompleteResponse::new();
    }

    let feeds = service
        .search_and_combine_feeds(partial, user_sub, guild_sub)
        .await;

    if ctx.guild_id().is_none() && feeds.is_empty() {
        return CreateAutocompleteResponse::new().set_choices(vec![AutocompleteChoice::from(
            "You have no subscriptions yet. Subscribe first with `/subscribe` command",
        )]);
    }

    let mut choices: Vec<AutocompleteChoice> = feeds
        .into_iter()
        .map(|feed| AutocompleteChoice::new(feed.name, feed.source_url))
        .collect();

    choices.truncate(25);
    CreateAutocompleteResponse::new().set_choices(choices)
}
