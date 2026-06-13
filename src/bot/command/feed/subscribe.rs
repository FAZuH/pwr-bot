//! Feed subscribe subcommand.

use crate::bot::command::feed::SendInto;
use crate::bot::command::feed::get_or_create_subscriber;
use crate::bot::command::feed::process_subscription_batch;
use crate::bot::command::feed::verify_server_config;
use crate::bot::command::prelude::*;

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
    Router::new(ctx)
        .run(Navigation::FeedSubscribe { links, send_into })
        .await?;
    Ok(())
}

handler! { pub struct FeedSubscribeHandler {
    links: String,
    send_into: Option<SendInto>,
} }

#[async_trait::async_trait]
impl CommandHandler for FeedSubscribeHandler {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        self.host_ctx.defer().await?;

        let send_into = self.send_into.unwrap_or(SendInto::DM);
        let urls = parse_and_validate_urls(&self.links)?;

        verify_server_config(ctx, &send_into, true).await?;

        let subscriber = get_or_create_subscriber(ctx, &send_into).await?;
        Ok(process_subscription_batch(coordinator, &urls, &subscriber, true).await?)
    }
}

pub async fn autocomplete_supported_feeds<'a>(
    _ctx: Context<'_>,
    partial: &str,
) -> CreateAutocompleteResponse<'a> {
    let mut choices = vec![AutocompleteChoice::new("Supported feeds are:", "foo")];
    let platforms = [
        ("AniList (anilist.co)", "anilist.co"),
        ("MangaDex (mangadex.org)", "mangadex.org"),
        ("Comick (comick.io)", "comick.io"),
    ];

    for (name, domain) in &platforms {
        if partial.is_empty()
            || name.to_lowercase().contains(&partial.to_lowercase())
            || domain.contains(&partial.to_lowercase())
        {
            choices.push(AutocompleteChoice::new(*name, *domain));
        }
    }

    choices.truncate(25);
    CreateAutocompleteResponse::new().set_choices(choices)
}
