//! Feed subscription management commands.

use std::time::Duration;
use std::time::Instant;

use poise::ChoiceParameter;
use serenity::all::ButtonStyle;
use serenity::all::ComponentInteraction;
use serenity::all::CreateActionRow;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateTextDisplay;
use serenity::all::GuildId;
use serenity::all::RoleId;
use serenity::all::UserId;

use crate::action_enum;
use crate::bot::checks::check_author_roles;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::settings::SettingsPage;
use crate::bot::commands::settings::run_settings;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::InteractiveView;
use crate::bot::views::InteractiveViewBase;
use crate::bot::views::RenderExt;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::bot::views::ViewCommand;
use crate::bot::views::ViewCore;
use crate::bot::views::ViewHandler;
use crate::entity::SubscriberEntity;
use crate::entity::SubscriberType;
use crate::service::feed_subscription_service::SubscribeResult;
use crate::service::feed_subscription_service::SubscriberTarget;
use crate::service::feed_subscription_service::UnsubscribeResult;

pub mod list;
pub mod settings;
pub mod subscribe;
pub mod unsubscribe;

/// Manage feed subscriptions and settings
///
/// Base command for feed management. Use subcommands to:
/// - Subscribe to feeds
/// - Unsubscribe from feeds
/// - View your subscriptions
/// - Configure server feed settings (admin only)
#[poise::command(
    slash_command,
    subcommands(
        "settings::settings",
        "subscribe::subscribe",
        "unsubscribe::unsubscribe",
        "list::list"
    )
)]
pub async fn feed(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Update interval for batch processing in seconds.
const UPDATE_INTERVAL_SECS: u64 = 2;

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

/// Legacy function for feed settings command.
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    run_settings(ctx, Some(SettingsPage::Feeds)).await
}

/// Processes a batch of subscription/unsubscription operations.
async fn process_subscription_batch(
    ctx: Context<'_>,
    urls: &[&str],
    subscriber: &SubscriberEntity,
    is_subscribe: bool,
) -> Result<NavigationResult, Error> {
    let mut states: Vec<String> = vec!["⏳ Processing...".to_string(); urls.len()];
    let mut last_send = Instant::now();
    let mut view: Option<FeedSubscriptionBatchView> = None;
    let service = ctx.data().service.feed_subscription.clone();

    for (i, url) in urls.iter().enumerate() {
        let result_str = if is_subscribe {
            service
                .subscribe(url, subscriber)
                .await
                .map(|res| res.into())
        } else {
            service
                .unsubscribe(url, subscriber)
                .await
                .map(|res| res.into())
        };

        states[i] = result_str.unwrap_or_else(|e| format!("❌ {e}"));

        let is_final = i + 1 == urls.len();
        if last_send.elapsed().as_secs() > UPDATE_INTERVAL_SECS || is_final {
            let mut batch_view = FeedSubscriptionBatchView::new(&ctx, states.clone(), is_final);
            batch_view.render().await?;
            if is_final {
                view = Some(batch_view);
            }
            last_send = Instant::now();
        }
    }

    // Listen for "View Subscriptions" button click after final message
    if let Some(mut view) = view {
        let nav = std::sync::Arc::new(std::sync::Mutex::new(NavigationResult::Exit));

        view.run(|action| {
            let nav = nav.clone();
            let subscriber_type = subscriber.r#type;
            Box::pin(async move {
                if action == FeedSubscriptionBatchAction::ViewSubscriptions {
                    // Convert subscriber type back to SendInto
                    let send_into = match subscriber_type {
                        SubscriberType::Guild => SendInto::Server,
                        SubscriberType::Dm => SendInto::DM,
                    };
                    *nav.lock().unwrap() = NavigationResult::FeedList(Some(send_into));
                    return ViewCommand::Exit;
                }
                ViewCommand::Render
            })
        })
        .await?;

        let res = nav.lock().unwrap().clone();
        Ok(res)
    } else {
        Ok(NavigationResult::Exit)
    }
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
) -> Result<SubscriberEntity, Error> {
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

action_enum! { FeedSubscriptionBatchAction {
    #[label = "View Subscriptions"]
    ViewSubscriptions,
} }

pub struct FeedSubscriptionBatchHandler {
    pub states: Vec<String>,
    pub is_final: bool,
}

#[async_trait::async_trait]
impl ViewHandler<FeedSubscriptionBatchAction> for FeedSubscriptionBatchHandler {
    async fn handle(
        &mut self,
        action: &FeedSubscriptionBatchAction,
        _interaction: &ComponentInteraction,
    ) -> Result<Option<FeedSubscriptionBatchAction>, Error> {
        match action {
            FeedSubscriptionBatchAction::ViewSubscriptions => Ok(Some(action.clone())),
        }
    }
}

pub struct FeedSubscriptionBatchView<'a> {
    pub base: InteractiveViewBase<'a, FeedSubscriptionBatchAction>,
    pub handler: FeedSubscriptionBatchHandler,
}

impl<'a> View<'a, FeedSubscriptionBatchAction> for FeedSubscriptionBatchView<'a> {
    fn core(&self) -> &ViewCore<'a, FeedSubscriptionBatchAction> {
        &self.base.core
    }
    fn core_mut(&mut self) -> &mut ViewCore<'a, FeedSubscriptionBatchAction> {
        &mut self.base.core
    }
    fn create_core(poise_ctx: &'a Context<'a>) -> ViewCore<'a, FeedSubscriptionBatchAction> {
        ViewCore::new(poise_ctx, Duration::from_secs(120))
    }
}

impl<'a> FeedSubscriptionBatchView<'a> {
    /// Creates a new batch view with the given states.
    pub fn new(ctx: &'a Context<'a>, states: Vec<String>, is_final: bool) -> Self {
        Self {
            base: InteractiveViewBase::new(Self::create_core(ctx)),
            handler: FeedSubscriptionBatchHandler { states, is_final },
        }
    }
}

impl<'a> ResponseView<'a> for FeedSubscriptionBatchView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let text_components: Vec<CreateContainerComponent> = self
            .handler
            .states
            .iter()
            .map(|s| CreateContainerComponent::TextDisplay(CreateTextDisplay::new(s.clone())))
            .collect();

        let mut components = vec![CreateComponent::Container(CreateContainer::new(
            text_components,
        ))];

        if self.handler.is_final {
            let nav_button = self
                .base
                .register(FeedSubscriptionBatchAction::ViewSubscriptions)
                .as_button()
                .style(ButtonStyle::Secondary);

            components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
                vec![nav_button].into(),
            )));
        }

        components.into()
    }
}

crate::impl_interactive_view!(
    FeedSubscriptionBatchView<'a>,
    FeedSubscriptionBatchHandler,
    FeedSubscriptionBatchAction
);

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
