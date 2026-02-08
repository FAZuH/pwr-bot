use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::feeds::commands::SendInto;

pub mod commands;
pub mod views;

pub struct FeedsCog;

impl FeedsCog {
    #[poise::command(
        slash_command,
        subcommands(
            "Self::settings",
            "Self::subscribe",
            "Self::unsubscribe",
            "Self::subscriptions"
        )
    )]
    pub async fn feed(_ctx: Context<'_>) -> Result<(), Error> {
        Ok(())
    }

    #[poise::command(
        slash_command,
        default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
    )]
    pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
        commands::settings(ctx).await
    }

    #[poise::command(slash_command)]
    pub async fn subscribe(
        ctx: Context<'_>,
        #[description = "Link(s) of the feeds. Separate links with commas (,)"]
        #[autocomplete = "Self::autocomplete_supported_feeds"]
        links: String,
        #[description = "Where to send the notifications. Default to your DM"] send_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        commands::subscribe(ctx, links, send_into).await
    }

    #[poise::command(slash_command)]
    pub async fn unsubscribe(
        ctx: Context<'_>,
        #[description = "Link(s) of the feeds. Separate links with commas (,)"]
        #[autocomplete = "Self::autocomplete_subscriptions"]
        links: String,
        #[description = "Where notifications were being sent. Default to DM"] send_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        commands::unsubscribe(ctx, links, send_into).await
    }

    #[poise::command(slash_command)]
    pub async fn subscriptions(
        ctx: Context<'_>,
        #[description = "Where the notifications are being sent. Default to DM"] sent_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        commands::subscriptions(ctx, sent_into).await
    }

    async fn autocomplete_subscriptions<'a>(
        ctx: Context<'_>,
        partial: &str,
    ) -> poise::serenity_prelude::CreateAutocompleteResponse<'a> {
        commands::autocomplete_subscriptions(ctx, partial).await
    }

    async fn autocomplete_supported_feeds<'a>(
        ctx: Context<'_>,
        partial: &str,
    ) -> poise::serenity_prelude::CreateAutocompleteResponse<'a> {
        commands::autocomplete_supported_feeds(ctx, partial).await
    }
}

impl Cog for FeedsCog {
    fn commands(&self) -> Vec<poise::Command<crate::bot::Data, super::Error>> {
        vec![Self::feed()]
    }
}
