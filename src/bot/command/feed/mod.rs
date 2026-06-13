//! Feed subscription management commands.

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::bot::checks::check_author_roles;
use crate::bot::command::prelude::*;
use crate::entity::FeedEntity;
use crate::entity::SubscriberEntity;
use crate::entity::SubscriberType;

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

// -- Result types for subscribe/unsubscribe operations --
// These were previously in service::feed_subscription but are now local to
// the feed command module. The actual operations are handled by the feed plugin.

pub enum SubscribeResult {
    Success { feed: FeedEntity },
    AlreadySubscribed { feed: FeedEntity },
}

pub enum UnsubscribeResult {
    Success { feed: FeedEntity },
    AlreadyUnsubscribed { feed: FeedEntity },
    NoneSubscribed { url: String },
}

#[derive(Debug, Clone)]
pub struct SubscriberTarget {
    pub subscriber_type: SubscriberType,
    pub target_id: String,
}

/// Where to send feed notifications.
#[derive(ChoiceParameter, Clone, Copy, Debug, PartialEq, Eq)]
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
                format!("❌ You are **not subscribed** to <{url}>")
            }
        }
    }
}

/// Processes a batch of subscription/unsubscription operations.
///
/// The actual subscribe/unsubscribe operations are handled by the feed plugin.
#[allow(unused_variables)]
async fn process_subscription_batch(
    coordinator: Arc<Router<'_>>,
    urls: &[&str],
    subscriber: &SubscriberEntity,
    is_subscribe: bool,
) -> Result<(), Error> {
    let mut states: Vec<String> = vec!["⏳ Processing...".to_string(); urls.len()];
    let mut last_send = Instant::now();
    let mut handler: Option<FeedSubscriptionBatchHandler> = None;
    let ctx = coordinator.context();

    for (i, url) in urls.iter().enumerate() {
        states[i] = format!(
            "{}",
            if is_subscribe {
                "ℹ️ Subscribe: handled by feed plugin"
            } else {
                "ℹ️ Unsubscribe: handled by feed plugin"
            }
        );

        let is_final = i + 1 == urls.len();
        if last_send.elapsed().as_secs() > UPDATE_INTERVAL_SECS || is_final {
            let batch_handler = FeedSubscriptionBatchHandler {
                states: states.clone(),
                is_final,
            };

            let mut engine = ViewEngine::new(
                *ctx,
                batch_handler,
                Duration::from_millis(1),
                coordinator.clone(),
            );

            if !is_final {
                engine.run().await?;
            } else {
                handler = Some(engine.handler);
            }
            last_send = Instant::now();
        }
    }

    if let Some(handler) = handler {
        let mut engine =
            ViewEngine::new(*ctx, handler, Duration::from_secs(120), coordinator.clone());

        engine.run().await?;
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
            .settings
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
#[allow(dead_code)]
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
    _ctx: Context<'_>,
    _send_into: &SendInto,
) -> Result<SubscriberEntity, Error> {
    Err("Subscriber management is handled by the feed plugin".into())
}

/// Placeholder action type for the non-interactive batch handler.
#[derive(Debug, Clone)]
pub enum FeedSubscriptionBatchAction {}

impl Action for FeedSubscriptionBatchAction {
    fn label(&self) -> &'static str {
        match *self {}
    }
}

pub struct FeedSubscriptionBatchHandler {
    pub states: Vec<String>,
    pub is_final: bool,
}

#[async_trait::async_trait]
impl ViewHandler for FeedSubscriptionBatchHandler {
    type Action = FeedSubscriptionBatchAction;
    async fn handle(
        &mut self,
        _ctx: ViewContext<'_, FeedSubscriptionBatchAction>,
    ) -> Result<ViewCmd, Error> {
        unreachable!()
    }
}

impl ViewRender for FeedSubscriptionBatchHandler {
    type Action = FeedSubscriptionBatchAction;
    fn render(
        &self,
        _registry: &mut ActionRegistry<FeedSubscriptionBatchAction>,
    ) -> ResponseKind<'_> {
        let mut text_components: Vec<CreateContainerComponent> = self
            .states
            .iter()
            .map(|s| CreateContainerComponent::TextDisplay(CreateTextDisplay::new(s.clone())))
            .collect();

        if self.is_final {
            text_components.push(CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new("Use `/feed list` to view your subscriptions."),
            ));
        }

        vec![CreateComponent::Container(CreateContainer::new(
            text_components,
        ))]
        .into()
    }
}

#[cfg(test)]
mod tests {
    use poise::serenity_prelude::GuildId;
    use poise::serenity_prelude::UserId;

    use super::*;

    #[test]
    fn send_into_to_subscriber_type() {
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
    fn send_into_display() {
        assert_eq!(SendInto::DM.to_string(), "dm");
        assert_eq!(SendInto::Server.to_string(), "server");
    }

    #[test]
    fn get_target_id_dm_returns_author_id() {
        let result = get_target_id(Some(GuildId::new(999)), UserId::new(12345), &SendInto::DM);
        assert_eq!(result.unwrap(), "12345");
    }

    #[test]
    fn get_target_id_server_returns_guild_id() {
        let result = get_target_id(
            Some(GuildId::new(999)),
            UserId::new(12345),
            &SendInto::Server,
        );
        assert_eq!(result.unwrap(), "999");
    }

    #[test]
    fn get_target_id_server_without_guild_fails() {
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
