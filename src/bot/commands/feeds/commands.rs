use std::time::Duration;

use poise::ChoiceParameter;
use serenity::all::ComponentInteractionCollector;
use serenity::all::CreateInteractionResponse;
use serenity::all::CreateInteractionResponseMessage;
use serenity::all::GuildId;
use serenity::all::RoleId;
use serenity::all::UserId;
use serenity::futures::StreamExt;

use crate::bot::checks::check_author_roles;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::feeds::views::SettingsFeedsView;
use crate::bot::commands::feeds::views::SubscriptionBatchView;
use crate::bot::commands::feeds::views::SubscriptionsListView;
use crate::bot::error::BotError;
use crate::bot::utils::parse_and_validate_urls;
use crate::bot::views::pagination::PaginationHandler;
use crate::bot::views::pagination::PaginationState;
use crate::database::model::FeedModel;
use crate::database::model::ServerSettings;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::service::feed_subscription_service::SubscribeResult;
use crate::service::feed_subscription_service::SubscriberTarget;
use crate::service::feed_subscription_service::UnsubscribeResult;

const INTERACTION_TIMEOUT_SECS: u64 = 120;
const UPDATE_INTERVAL_SECS: u64 = 2;

#[derive(ChoiceParameter)]
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

pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer().await?;
    let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

    let mut settings = ctx
        .data()
        .service
        .feed_subscription
        .get_server_settings(guild_id)
        .await?;

    let msg_handle = ctx.send(SettingsFeedsView::create_reply(&settings)).await?;

    let msg_id = msg_handle.message().await?.into_owned().id;
    let author_id = ctx.author().id;

    let mut collector = ComponentInteractionCollector::new(ctx.serenity_context())
        .message_id(msg_id)
        .author_id(author_id)
        .timeout(Duration::from_secs(INTERACTION_TIMEOUT_SECS))
        .stream();

    while let Some(interaction) = collector.next().await {
        let should_update = apply_settings_interaction(&mut settings, &interaction);

        if should_update {
            ctx.data()
                .service
                .feed_subscription
                .update_server_settings(guild_id, settings.clone())
                .await?;
        }

        interaction
            .create_response(
                ctx.http(),
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .components(SettingsFeedsView::create_components(&settings)),
                ),
            )
            .await?;
    }

    Ok(())
}

fn apply_settings_interaction(
    settings: &mut ServerSettings,
    interaction: &serenity::all::ComponentInteraction,
) -> bool {
    let custom_id = &interaction.data.custom_id;
    match &interaction.data.kind {
        serenity::all::ComponentInteractionDataKind::StringSelect { values }
            if custom_id == "server_settings_enabled" =>
        {
            if let Some(value) = values.first() {
                settings.enabled = Some(value == "true");
                return true;
            }
        }
        serenity::all::ComponentInteractionDataKind::ChannelSelect { values }
            if custom_id == "server_settings_channel" =>
        {
            settings.channel_id = values.first().map(|id| id.to_string());
            return true;
        }
        serenity::all::ComponentInteractionDataKind::RoleSelect { values }
            if custom_id == "server_settings_sub_role" =>
        {
            settings.subscribe_role_id = if values.is_empty() {
                None
            } else {
                values.first().map(|id| id.to_string())
            };
            return true;
        }
        serenity::all::ComponentInteractionDataKind::RoleSelect { values }
            if custom_id == "server_settings_unsub_role" =>
        {
            settings.unsubscribe_role_id = if values.is_empty() {
                None
            } else {
                values.first().map(|id| id.to_string())
            };
            return true;
        }
        _ => {}
    }
    false
}

pub async fn subscribe(
    ctx: Context<'_>,
    links: String,
    send_into: Option<SendInto>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let send_into = send_into.unwrap_or(SendInto::DM);
    let urls = parse_and_validate_urls(&links)?;

    verify_server_config(ctx, &send_into, true).await?;

    let subscriber = get_or_create_subscriber(ctx, &send_into).await?;
    process_subscription_batch(ctx, &urls, &subscriber, true).await?;

    Ok(())
}

pub async fn unsubscribe(
    ctx: Context<'_>,
    links: String,
    send_into: Option<SendInto>,
) -> Result<(), Error> {
    ctx.defer().await?;

    let send_into = send_into.unwrap_or(SendInto::DM);
    let urls = parse_and_validate_urls(&links)?;

    verify_server_config(ctx, &send_into, false).await?;

    let subscriber = get_or_create_subscriber(ctx, &send_into).await?;
    process_subscription_batch(ctx, &urls, &subscriber, false).await?;

    Ok(())
}

pub async fn subscriptions(ctx: Context<'_>, sent_into: Option<SendInto>) -> Result<(), Error> {
    ctx.defer().await?;
    let sent_into = sent_into.unwrap_or(SendInto::DM);

    let subscriber = get_or_create_subscriber(ctx, &sent_into).await?;

    let total_items = ctx
        .data()
        .service
        .feed_subscription
        .get_subscription_count(&subscriber)
        .await?;

    let pages = total_items.div_ceil(10);
    let state = PaginationState::new(pages, 10, 1);
    let mut pagination = PaginationHandler::new(&ctx, state);
    let view = SubscriptionsListView;

    let subscriptions = ctx
        .data()
        .service
        .feed_subscription
        .list_paginated_subscriptions(&subscriber, 1u32, 10u32)
        .await?;

    let reply = view.create_reply(subscriptions);
    let msg_handle = ctx.send(reply).await?;

    while pagination.listen(Duration::from_secs(60)).await.is_some() {
        let current_page = pagination.state().current_page;

        let subscriptions = ctx
            .data()
            .service
            .feed_subscription
            .list_paginated_subscriptions(&subscriber, current_page, 10u32)
            .await?;

        let components = view.create_page(subscriptions);
        let reply = poise::CreateReply::new()
            .flags(serenity::all::MessageFlags::IS_COMPONENTS_V2)
            .components(components);

        msg_handle.edit(ctx, reply).await?;
    }

    Ok(())
}

async fn process_subscription_batch(
    ctx: Context<'_>,
    urls: &[&str],
    subscriber: &SubscriberModel,
    is_subscribe: bool,
) -> Result<(), Error> {
    use std::time::Instant;

    let mut states: Vec<String> = vec!["⏳ Processing...".to_string(); urls.len()];
    let mut last_send = Instant::now();
    let mut msg_handle: Option<poise::ReplyHandle<'_>> = None;

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
            let resp = SubscriptionBatchView::create(&states, is_final);
            match msg_handle {
                None => msg_handle = Some(ctx.send(resp).await?),
                Some(ref handle) => handle.edit(ctx, resp).await?,
            }
            last_send = Instant::now();
        }
    }
    Ok(())
}

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

        if settings.channel_id.is_none() {
            return Err(BotError::ConfigurationError(
                "Server feed settings are not configured. A server admin must run `/settings` to configure a notification channel first.".to_string(),
            ).into());
        }

        let role_id = if is_subscribe {
            &settings.subscribe_role_id
        } else {
            &settings.unsubscribe_role_id
        };

        let role_id = match role_id.as_ref() {
            Some(id) => vec![RoleId::new(id.parse()?)],
            None => vec![],
        };

        check_author_roles(ctx, role_id).await?;
    }
    Ok(())
}

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
) -> poise::serenity_prelude::CreateAutocompleteResponse<'a> {
    let mut choices = vec![poise::serenity_prelude::AutocompleteChoice::new(
        "Supported feeds are:",
        "foo",
    )];
    let feeds = ctx.data().platforms.get_all_platforms();

    for feed in feeds {
        let info = &feed.get_base().info;
        let name = format!("{} ({})", info.name, info.api_domain);
        if partial.is_empty()
            || name.to_lowercase().contains(&partial.to_lowercase())
            || info.api_domain.contains(&partial.to_lowercase())
        {
            choices.push(poise::serenity_prelude::AutocompleteChoice::new(
                name,
                info.api_domain.clone(),
            ));
        }
    }

    choices.truncate(25);
    poise::serenity_prelude::CreateAutocompleteResponse::new().set_choices(choices)
}

pub async fn autocomplete_subscriptions<'a>(
    ctx: Context<'_>,
    partial: &str,
) -> poise::serenity_prelude::CreateAutocompleteResponse<'a> {
    if partial.trim().is_empty() {
        return poise::serenity_prelude::CreateAutocompleteResponse::new().set_choices(vec![
            poise::serenity_prelude::AutocompleteChoice::from("Start typing to see suggestions"),
        ]);
    }

    let (user_sub, guild_sub) = get_both_subscribers(ctx).await;

    if user_sub.is_none() && guild_sub.is_none() {
        return poise::serenity_prelude::CreateAutocompleteResponse::new();
    }

    let feeds = search_and_combine_feeds(ctx, partial, user_sub, guild_sub).await;

    if ctx.guild_id().is_none() && feeds.is_empty() {
        return poise::serenity_prelude::CreateAutocompleteResponse::new().set_choices(vec![
            poise::serenity_prelude::AutocompleteChoice::from(
                "You have no subscriptions yet. Subscribe first with `/subscribe` command",
            ),
        ]);
    }

    let mut choices: Vec<poise::serenity_prelude::AutocompleteChoice> = feeds
        .into_iter()
        .map(|feed| poise::serenity_prelude::AutocompleteChoice::new(feed.name, feed.source_url))
        .collect();

    choices.truncate(25);
    poise::serenity_prelude::CreateAutocompleteResponse::new().set_choices(choices)
}

async fn search_and_combine_feeds(
    ctx: Context<'_>,
    partial: &str,
    user_subscriber: Option<SubscriberModel>,
    guild_subscriber: Option<SubscriberModel>,
) -> Vec<crate::database::model::FeedModel> {
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
