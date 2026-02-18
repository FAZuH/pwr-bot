//! Feed subscription management commands.

use crate::bot::commands::Cog;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
pub use crate::bot::commands::feed::controllers::SendInto;

pub mod controllers;
pub mod views;

/// Cog for feed subscription commands.
pub struct FeedCog;

impl FeedCog {
    /// Manage feed subscriptions and settings
    ///
    /// Base command for feed management. Use subcommands to:
    /// - Subscribe to feeds
    /// - Unsubscribe from feeds
    /// - View your subscriptions
    /// - Configure server feed settings (admin only)
    #[poise::command(
        slash_command,
        subcommands("Self::settings", "Self::subscribe", "Self::unsubscribe", "Self::list")
    )]
    pub async fn feed(_ctx: Context<'_>) -> Result<(), Error> {
        Ok(())
    }

    /// Configure feed settings for this server
    ///
    /// Set up notification channels and required roles for feed subscriptions.
    /// Only server administrators can use this command.
    #[poise::command(
        slash_command,
        default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
    )]
    pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
        controllers::settings(ctx).await
    }

    /// Subscribe to one or more feeds
    ///
    /// Add feeds to receive notifications. You can subscribe in your DM or
    /// in the server (if server feed settings are configured).
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
        controllers::subscribe(ctx, links, send_into).await
    }

    /// Unsubscribe from one or more feeds
    ///
    /// Remove feeds from your subscriptions. Use autocomplete to find
    /// feeds you are currently subscribed to.
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
        controllers::unsubscribe(ctx, links, send_into).await
    }

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
        controllers::list(ctx, sent_into).await
    }

    async fn autocomplete_subscriptions<'a>(
        ctx: Context<'_>,
        partial: &str,
    ) -> poise::serenity_prelude::CreateAutocompleteResponse<'a> {
        controllers::autocomplete_subscriptions(ctx, partial).await
    }

    async fn autocomplete_supported_feeds<'a>(
        ctx: Context<'_>,
        partial: &str,
    ) -> poise::serenity_prelude::CreateAutocompleteResponse<'a> {
        controllers::autocomplete_supported_feeds(ctx, partial).await
    }
}

impl Cog for FeedCog {
    fn commands(&self) -> Vec<poise::Command<crate::bot::Data, super::Error>> {
        vec![Self::feed()]
    }
}
