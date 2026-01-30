use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::feeds::controller::SettingsController;
use crate::bot::commands::feeds::controller::SubscribeController;
use crate::bot::commands::feeds::controller::SubscriptionsController;
use crate::bot::commands::feeds::controller::UnsubscribeController;
use crate::bot::commands::feeds::model::SendInto;

pub mod controller;
pub mod model;
pub mod view;

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
        SettingsController::execute(ctx).await
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
        SubscribeController::execute(ctx, links, send_into).await
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
        UnsubscribeController::execute(ctx, links, send_into).await
    }

    #[poise::command(slash_command)]
    pub async fn subscriptions(
        ctx: Context<'_>,
        #[description = "Where the notifications are being sent. Default to DM"] sent_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        SubscriptionsController::execute(ctx, sent_into).await
    }

    async fn autocomplete_subscriptions<'a>(
        ctx: Context<'_>,
        partial: &str,
    ) -> poise::serenity_prelude::CreateAutocompleteResponse<'a> {
        UnsubscribeController::autocomplete_subscriptions(ctx, partial).await
    }

    async fn autocomplete_supported_feeds<'a>(
        ctx: Context<'_>,
        partial: &str,
    ) -> poise::serenity_prelude::CreateAutocompleteResponse<'a> {
        SubscribeController::autocomplete_supported_feeds(ctx, partial).await
    }
}

impl Cog for FeedsCog {
    fn commands(&self) -> Vec<poise::Command<crate::bot::Data, super::Error>> {
        vec![Self::feed()]
    }
}
