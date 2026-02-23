//! Feed subscribe subcommand.

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

/// Subscribe to one or more feeds
///
/// Add feeds to receive notifications. You can subscribe in your DM or
/// in the server (if server feed settings are configured).
#[poise::command(slash_command)]
pub async fn subscribe(
    ctx: Context<'_>,
    #[description = "Link(s) of the feeds. Separate links with commas (,)"]
    #[autocomplete = "autocomplete_supported_feeds"]
    links: String,
    #[description = "Where to send the notifications. Default to your DM"] send_into: Option<
        SendInto,
    >,
) -> Result<(), Error> {
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = FeedSubscribeController::new(&ctx, links, send_into);
    let _result = controller.run(&mut coordinator).await?;
    Ok(())
}

controller! { pub struct FeedSubscribeController<'a> {
    links: String,
    send_into: Option<SendInto>,
} }

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for FeedSubscribeController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let send_into = self.send_into.unwrap_or(SendInto::DM);
        let urls = parse_and_validate_urls(&self.links)?;

        verify_server_config(ctx, &send_into, true).await?;

        let subscriber = get_or_create_subscriber(ctx, &send_into).await?;
        Ok(process_subscription_batch(ctx, &urls, &subscriber, true).await?)
    }
}

pub async fn autocomplete_supported_feeds<'a>(
    ctx: Context<'_>,
    partial: &str,
) -> CreateAutocompleteResponse<'a> {
    let mut choices = vec![AutocompleteChoice::new("Supported feeds are:", "foo")];
    let feeds = ctx.data().platforms.get_all_platforms();

    for feed in feeds {
        let info = &feed.get_base().info;
        let name = format!("{} ({})", info.name, info.api_domain);
        if partial.is_empty()
            || name.to_lowercase().contains(&partial.to_lowercase())
            || info.api_domain.contains(&partial.to_lowercase())
        {
            choices.push(AutocompleteChoice::new(name, info.api_domain.clone()));
        }
    }

    choices.truncate(25);
    CreateAutocompleteResponse::new().set_choices(choices)
}
