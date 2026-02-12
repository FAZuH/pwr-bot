//! Command implementations for feed management.

use std::time::Instant;

use poise::ChoiceParameter;
use poise::CreateReply;
use poise::ReplyHandle;
use serenity::all::AutocompleteChoice;
use serenity::all::CreateAutocompleteResponse;
use serenity::all::CreateInteractionResponse;
use serenity::all::CreateInteractionResponseMessage;
use serenity::all::GuildId;
use serenity::all::MessageFlags;
use serenity::all::RoleId;
use serenity::all::UserId;

use crate::bot::checks::check_author_roles;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::feed::views::FeedSubscriptionBatchAction;
use crate::bot::commands::feed::views::FeedSubscriptionBatchView;
use crate::bot::commands::feed::views::FeedSubscriptionsListView;
use crate::bot::commands::feed::views::SettingsFeedAction;
use crate::bot::commands::feed::views::SettingsFeedView;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::utils::parse_and_validate_urls;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseComponentView;
use crate::bot::views::pagination::PaginationView;
use crate::database::model::FeedModel;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::service::feed_subscription_service::SubscribeResult;
use crate::service::feed_subscription_service::SubscriberTarget;
use crate::service::feed_subscription_service::UnsubscribeResult;

/// Update interval for batch processing in seconds.
const UPDATE_INTERVAL_SECS: u64 = 2;

/// Number of items per page for subscriptions list.
const SUBSCRIPTIONS_PER_PAGE: u32 = 10;

/// Where to send feed notifications.
#[derive(ChoiceParameter, Clone, Copy, Debug)]
pub enum SendInto {
    Server,
    DM,
}

impl From<&SendInto> for SubscriberType {
    fn from(value: &SendInto) -> Self {
        match value {
            SendInto::DM => SubscriberType::Dm,
            SendInto::Server => SubscriberType::Guild,
        }
    }
}

impl std::fmt::Display for SendInto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DM => write!(f, "dm"),
            Self::Server => write!(f, "server"),
        }
    }
}

impl SendInto {
    /// Returns the display name for this send target.
    pub fn name(&self) -> &'static str {
        match self {
            Self::DM => "DM",
            Self::Server => "Server",
        }
    }
}

impl From<SubscribeResult> for String {
    fn from(value: SubscribeResult) -> String {
        match value {
            SubscribeResult::Success { feed } => {
                format!(
                    "✅ **Successfully** subscribed to [{}](<{}>)",
                    feed.name, feed.source_url
                )
            }
            SubscribeResult::AlreadySubscribed { feed } => {
                format!(
                    "❌ You are **already subscribed** to [{}](<{}>)",
                    feed.name, feed.source_url
                )
            }
        }
    }
}

impl From<UnsubscribeResult> for String {
    fn from(value: UnsubscribeResult) -> Self {
        match value {
            UnsubscribeResult::Success { feed } => {
                format!(
                    "✅ **Successfully** unsubscribed from [{}](<{}>)",
                    feed.name, feed.source_url
                )
            }
            UnsubscribeResult::AlreadyUnsubscribed { feed } => {
                format!(
                    "❌ You are **not subscribed** to [{}](<{}>)",
                    feed.name, feed.source_url
                )
            }
            UnsubscribeResult::NoneSubscribed { url } => {
                format!("❌ You are **not subscribed** to <{}>", url)
            }
        }
    }
}

/// Controller for feed settings.
pub struct FeedSettingsController<'a> {
    ctx: &'a Context<'a>,
}

impl<'a> FeedSettingsController<'a> {
    /// Creates a new feed settings controller.
    pub fn new(ctx: &'a Context<'a>) -> Self {
        Self { ctx }
    }
}

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for FeedSettingsController<'a> {
    async fn run(&mut self, coordinator: &mut Coordinator<'_, S>) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;
        let guild_id = ctx
            .guild_id()
            .ok_or(BotError::GuildOnlyCommand)?
            .get();

        let mut settings = ctx
            .data()
            .service
            .feed_subscription
            .get_server_settings(guild_id)
            .await?;

        let mut view = SettingsFeedView::new(&ctx, &mut settings);
        coordinator.send(view.create_reply()).await?;

        while let Some((action, interaction)) = view.listen_once().await {
            if action == SettingsFeedAction::Back {
                return Ok(NavigationResult::Back);
            } else if action == SettingsFeedAction::About {
                return Ok(NavigationResult::SettingsAbout);
            }

            ctx
                .data()
                .service
                .feed_subscription
                .update_server_settings(guild_id, view.settings.clone())
                .await?;

            let reply = CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .flags(MessageFlags::IS_COMPONENTS_V2)
                    .components(view.create_components()),
            );

            interaction
                .create_response(ctx.http(), reply)
                .await?;
        }

        Ok(NavigationResult::Exit)
    }
}

/// Controller for subscriptions list with pagination.
pub struct FeedSubscriptionsController<'a> {
    ctx: &'a Context<'a>,
    send_into: SendInto,
}

impl<'a> FeedSubscriptionsController<'a> {
    /// Creates a new subscriptions controller.
    pub fn new(ctx: &'a Context<'a>, send_into: SendInto) -> Self {
        Self { ctx, send_into }
    }
}

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for FeedSubscriptionsController<'a> {
    async fn run(&mut self, coordinator: &mut Coordinator<'_, S>) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let subscriber = get_or_create_subscriber(ctx, &self.send_into).await?;

        let total_items = ctx
            .data()
            .service
            .feed_subscription
            .get_subscription_count(&subscriber)
            .await?;

        let subscriptions = ctx
            .data()
            .service
            .feed_subscription
            .list_paginated_subscriptions(&subscriber, 1u32, SUBSCRIPTIONS_PER_PAGE)
            .await?;

        let mut view = FeedSubscriptionsListView::new(subscriptions);
        let mut pagination = PaginationView::new(&ctx, total_items, SUBSCRIPTIONS_PER_PAGE);

        let mut components = view.create_components();
        pagination.attach_if_multipage(&mut components);

        let msg = CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(components);

        let msg_handle = ctx.send(msg).await?;

        while (pagination.listen_once().await).is_some() {
            let subscriptions = ctx
                .data()
                .service
                .feed_subscription
                .list_paginated_subscriptions(
                    &subscriber,
                    pagination.state.current_page,
                    SUBSCRIPTIONS_PER_PAGE,
                )
                .await?;

            view.set_subscriptions(subscriptions);

            let mut components = view.create_components();
            pagination.attach_if_multipage(&mut components);

            let msg = CreateReply::new()
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .components(components);

            msg_handle.edit(ctx, msg).await?;
        }

        Ok(NavigationResult::Exit)
    }
}

/// Controller for subscription batch operations.
pub struct FeedSubscribeController<'a> {
    ctx: Context<'a>,
    links: String,
    send_into: Option<SendInto>,
}

impl<'a> FeedSubscribeController<'a> {
    /// Creates a new subscribe controller.
    pub fn new(ctx: Context<'a>, links: String, send_into: Option<SendInto>) -> Self {
        Self {
            ctx,
            links,
            send_into,
        }
    }
}

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for FeedSubscribeController<'a> {
    async fn run(&mut self, coordinator: &mut Coordinator<'_, S>) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let send_into = self.send_into.unwrap_or(SendInto::DM);
        let urls = parse_and_validate_urls(&self.links)?;

        verify_server_config(ctx, &send_into, true).await?;

        let subscriber = get_or_create_subscriber(ctx, &send_into).await?;
        process_subscription_batch(ctx, &urls, &subscriber, true).await?;

        Ok(NavigationResult::Exit)
    }
}

/// Controller for unsubscription batch operations.
pub struct FeedUnsubscribeController<'a> {
    ctx: Context<'a>,
    links: String,
    send_into: Option<SendInto>,
}

impl<'a> FeedUnsubscribeController<'a> {
    /// Creates a new unsubscribe controller.
    pub fn new(ctx: Context<'a>, links: String, send_into: Option<SendInto>) -> Self {
        Self {
            ctx,
            links,
            send_into,
        }
    }
}

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for FeedUnsubscribeController<'a> {
    async fn run(&mut self, coordinator: &mut Coordinator<'_, S>) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let send_into = self.send_into.unwrap_or(SendInto::DM);
        let urls = parse_and_validate_urls(&self.links)?;

        verify_server_config(ctx, &send_into, false).await?;

        let subscriber = get_or_create_subscriber(ctx, &send_into).await?;
        process_subscription_batch(ctx, &urls, &subscriber, false).await?;

        Ok(NavigationResult::Exit)
    }
}

/// Legacy function for feed settings command.
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = FeedSettingsController::new(&ctx);
    let _result = controller.run(&mut coordinator).await?;
    Ok(())
}

/// Legacy function for subscribe command.
pub async fn subscribe(
    ctx: Context<'_>,
    links: String,
    send_into: Option<SendInto>,
) -> Result<(), Error> {
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = FeedSubscribeController::new(ctx, links, send_into);
    let _result = controller.run(&mut coordinator).await?;
    Ok(())
}

/// Legacy function for unsubscribe command.
pub async fn unsubscribe(
    ctx: Context<'_>,
    links: String,
    send_into: Option<SendInto>,
) -> Result<(), Error> {
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = FeedUnsubscribeController::new(ctx, links, send_into);
    let _result = controller.run(&mut coordinator).await?;
    Ok(())
}

/// Legacy function for subscriptions list command.
pub async fn subscriptions(ctx: Context<'_>, sent_into: Option<SendInto>) -> Result<(), Error> {
    let sent_into = sent_into.unwrap_or(SendInto::DM);
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = FeedSubscriptionsController::new(&ctx, sent_into);
    let _result = controller.run(&mut coordinator).await?;
    Ok(())
}

/// Processes a batch of subscription/unsubscription operations.
async fn process_subscription_batch(
    ctx: Context<'_>,
    urls: &[&str],
    subscriber: &SubscriberModel,
    is_subscribe: bool,
) -> Result<(), Error> {
    let mut states: Vec<String> = vec!["⏳ Processing...".to_string(); urls.len()];
    let mut last_send = Instant::now();
    let mut msg_handle: Option<ReplyHandle<'_>> = None;
    let mut view: Option<FeedSubscriptionBatchView> = None;

    for (i, url) in urls.iter().enumerate() {
        let result_str = if is_subscribe {
            ctx.data()
                .service
                .feed_subscription
                .subscribe(url, subscriber)
                .await
                .map(|res| res.into())
        } else {
            ctx.data()
                .service
                .feed_subscription
                .unsubscribe(url, subscriber)
                .await
                .map(|res| res.into())
        };

        states[i] = result_str.unwrap_or_else(|e| format!("❌ {e}"));

        let is_final = i + 1 == urls.len();
        if last_send.elapsed().as_secs() > UPDATE_INTERVAL_SECS || is_final {
            let batch_view = FeedSubscriptionBatchView::new(&ctx, states.clone(), is_final);
            let resp = batch_view.create_reply();
            match msg_handle {
                None => msg_handle = Some(ctx.send(resp).await?),
                Some(ref handle) => handle.edit(ctx, resp).await?,
            }
            if is_final {
                view = Some(batch_view);
            }
            last_send = Instant::now();
        }
    }

    // Listen for "View Subscriptions" button click after final message
    if let Some(mut view) = view
        && let Some((action, _)) = view.listen_once().await
        && action == FeedSubscriptionBatchAction::ViewSubscriptions
    {
        // Convert subscriber type back to SendInto
        let send_into = match subscriber.r#type {
            SubscriberType::Guild => SendInto::Server,
            SubscriberType::Dm => SendInto::DM,
        };
        subscriptions(ctx, Some(send_into)).await?;
    }

    Ok(())
}

/// Verifies server configuration is valid for the operation.
async fn verify_server_config(
    ctx: Context<'_>,
    send_into: &SendInto,
    is_subscribe: bool,
) -> Result<(), Error> {
    if let SendInto::Server = send_into {
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?;
        let settings = ctx
            .data()
            .service
            .feed_subscription
            .get_server_settings(guild_id.get())
            .await?;

        if settings.feeds.channel_id.is_none() {
            return Err(BotError::ConfigurationError(
                "Server feed settings are not configured. A server admin must run `/settings` to configure a notification channel first.".to_string(),
            ).into());
        }

        let role_id = if is_subscribe {
            &settings.feeds.subscribe_role_id
        } else {
            &settings.feeds.unsubscribe_role_id
        };

        let role_id = match role_id.as_ref() {
            Some(id) => vec![RoleId::new(id.parse()?)],
            None => vec![],
        };

        check_author_roles(ctx, role_id).await?;
    }
    Ok(())
}

/// Gets the target ID based on send target type.
fn get_target_id(
    guild_id: Option<GuildId>,
    author_id: UserId,
    send_into: &SendInto,
) -> Result<String, BotError> {
    match send_into {
        SendInto::Server => {
            let guild_id = guild_id.ok_or_else(|| BotError::InvalidCommandArgument {
                parameter: send_into.name().to_string(),
                reason: "You have to be in a server to do this command with send_into: server"
                    .to_string(),
            })?;
            Ok(guild_id.to_string())
        }
        SendInto::DM => Ok(author_id.to_string()),
    }
}

/// Gets or creates a subscriber for the current context.
async fn get_or_create_subscriber(
    ctx: Context<'_>,
    send_into: &SendInto,
) -> Result<SubscriberModel, Error> {
    let target_id = get_target_id(ctx.guild_id(), ctx.author().id, send_into)?;
    let subscriber_type = SubscriberType::from(send_into);
    let target = SubscriberTarget {
        subscriber_type,
        target_id,
    };
    Ok(ctx
        .data()
        .service
        .feed_subscription
        .get_or_create_subscriber(&target)
        .await?)
}

/// Gets both DM and guild subscribers for the current user.
async fn get_both_subscribers(
    ctx: Context<'_>,
) -> (Option<SubscriberModel>, Option<SubscriberModel>) {
    let user_target = SubscriberTarget {
        target_id: ctx.author().id.to_string(),
        subscriber_type: SubscriberType::Dm,
    };

    let user_subscriber = ctx
        .data()
        .service
        .feed_subscription
        .get_or_create_subscriber(&user_target)
        .await
        .ok();

    let guild_subscriber = match ctx.guild_id() {
        Some(guild_id) => {
            let guild_target = SubscriberTarget {
                target_id: guild_id.to_string(),
                subscriber_type: SubscriberType::Guild,
            };
            ctx.data()
                .service
                .feed_subscription
                .get_or_create_subscriber(&guild_target)
                .await
                .ok()
        }
        None => None,
    };

    (user_subscriber, guild_subscriber)
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

    let (user_sub, guild_sub) = get_both_subscribers(ctx).await;

    if user_sub.is_none() && guild_sub.is_none() {
        return CreateAutocompleteResponse::new();
    }

    let feeds = search_and_combine_feeds(ctx, partial, user_sub, guild_sub).await;

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

/// Searches and combines feeds from both user and guild subscriptions.
async fn search_and_combine_feeds(
    ctx: Context<'_>,
    partial: &str,
    user_subscriber: Option<SubscriberModel>,
    guild_subscriber: Option<SubscriberModel>,
) -> Vec<FeedModel> {
    let mut user_feeds = match user_subscriber {
        Some(sub) => ctx
            .data()
            .service
            .feed_subscription
            .search_subcriptions(&sub, partial)
            .await
            .unwrap_or_default(),
        None => vec![],
    };

    let mut guild_feeds = match guild_subscriber {
        Some(sub) => ctx
            .data()
            .service
            .feed_subscription
            .search_subcriptions(&sub, partial)
            .await
            .unwrap_or_default(),
        None => vec![],
    };

    for f in &mut user_feeds {
        format_subscription_with_prefix(f, true);
    }
    for f in &mut guild_feeds {
        format_subscription_with_prefix(f, false);
    }

    user_feeds.append(&mut guild_feeds);
    user_feeds
}

/// Adds a prefix to feed name indicating subscription type.
fn format_subscription_with_prefix(feed: &mut FeedModel, is_dm: bool) {
    let prefix = if is_dm { "(DM) " } else { "(Server) " };
    feed.name.insert_str(0, prefix);
}

#[cfg(test)]
mod tests {
    use serenity::all::GuildId;
    use serenity::all::UserId;

    use super::*;

    #[test]
    fn test_send_into_to_subscriber_type() {
        assert!(matches!(
            SubscriberType::from(&SendInto::DM),
            SubscriberType::Dm
        ));
        assert!(matches!(
            SubscriberType::from(&SendInto::Server),
            SubscriberType::Guild
        ));
    }

    #[test]
    fn test_send_into_display() {
        assert_eq!(SendInto::DM.to_string(), "dm");
        assert_eq!(SendInto::Server.to_string(), "server");
    }

    #[test]
    fn test_get_target_id_dm_returns_author_id() {
        let result = get_target_id(Some(GuildId::new(999)), UserId::new(12345), &SendInto::DM);
        assert_eq!(result.unwrap(), "12345");
    }

    #[test]
    fn test_get_target_id_server_returns_guild_id() {
        let result = get_target_id(
            Some(GuildId::new(999)),
            UserId::new(12345),
            &SendInto::Server,
        );
        assert_eq!(result.unwrap(), "999");
    }

    #[test]
    fn test_get_target_id_server_without_guild_fails() {
        let result = get_target_id(None, UserId::new(12345), &SendInto::Server);
        assert!(result.is_err());
        match result.unwrap_err() {
            BotError::InvalidCommandArgument { parameter, reason } => {
                assert_eq!(parameter, "Server");
                assert!(reason.contains("have to be in a server"));
            }
            _ => panic!("Expected InvalidCommandArgument error"),
        }
    }
}
