use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use poise::CreateReply;
use poise::ReplyHandle;
use poise::serenity_prelude::AutocompleteChoice;
use poise::serenity_prelude::CreateAutocompleteResponse;
use serenity::all::ComponentInteractionCollector;
use serenity::all::CreateInteractionResponse;
use serenity::all::CreateInteractionResponseMessage;
use serenity::all::GuildId;
use serenity::all::MessageFlags;
use serenity::all::RoleId;
use serenity::all::UserId;
use serenity::futures::StreamExt;

use crate::bot::checks::check_author_roles;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::feeds::model::SendInto;
use crate::bot::commands::feeds::view::SettingsFeedsView;
use crate::bot::commands::feeds::view::SubscriptionBatchView;
use crate::bot::commands::feeds::view::SubscriptionsListView;
use crate::bot::error::BotError;
use crate::bot::views::PageNavigationView;
use crate::bot::views::Pagination;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::service::feed_subscription_service::SubscriberTarget;

const INTERACTION_TIMEOUT_SECS: u64 = 120;
const UPDATE_INTERVAL_SECS: u64 = 2;
const MAX_URLS_PER_REQUEST: usize = 10;

pub struct SettingsController;

impl SettingsController {
    pub async fn execute(ctx: Context<'_>) -> Result<(), Error> {
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
            if settings.update(&interaction) {
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
}

pub struct SubscribeController;

impl SubscribeController {
    pub async fn execute(
        ctx: Context<'_>,
        links: String,
        send_into: Option<SendInto>,
    ) -> Result<(), Error> {
        ctx.defer().await?;

        let send_into = send_into.unwrap_or(SendInto::DM);
        let urls = parse_and_validate(&links)?;

        verify_server_config(ctx, &send_into, true).await?;

        let subscriber = get_subscriber(ctx, &send_into).await?;
        process_subscription_batch(ctx, &urls, &subscriber, true).await?;

        Ok(())
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
}

pub struct UnsubscribeController;

impl UnsubscribeController {
    pub async fn execute(
        ctx: Context<'_>,
        links: String,
        send_into: Option<SendInto>,
    ) -> Result<(), Error> {
        ctx.defer().await?;

        let send_into = send_into.unwrap_or(SendInto::DM);
        let urls = parse_and_validate(&links)?;

        verify_server_config(ctx, &send_into, false).await?;

        let subscriber = get_subscriber(ctx, &send_into).await?;
        process_subscription_batch(ctx, &urls, &subscriber, false).await?;

        Ok(())
    }

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
}

pub struct SubscriptionsController;

impl SubscriptionsController {
    pub async fn execute(ctx: Context<'_>, sent_into: Option<SendInto>) -> Result<(), Error> {
        ctx.defer().await?;
        let sent_into = sent_into.unwrap_or(SendInto::DM);

        let subscriber = get_subscriber(ctx, &sent_into).await?;

        // Pre-fetch data for pagination
        let total_items = ctx
            .data()
            .service
            .feed_subscription
            .get_subscription_count(&subscriber)
            .await?;

        let pages = total_items.div_ceil(10);
        let mut view = SubscriptionsListView::new(PageNavigationView::new(
            &ctx,
            Pagination::new(pages, 10, 1),
        ));

        // Fetch first page of subscriptions
        let subscriptions = ctx
            .data()
            .service
            .feed_subscription
            .list_paginated_subscriptions(&subscriber, 1u32, 10u32)
            .await?;

        let reply = ctx.send(view.create_reply(subscriptions)).await?;

        while view.navigation().listen(Duration::from_secs(60)).await {
            let current_page = view.navigation().pagination.current_page;

            // Fetch page data
            let subscriptions = ctx
                .data()
                .service
                .feed_subscription
                .list_paginated_subscriptions(&subscriber, current_page, 10u32)
                .await?;

            let components = view.create_page(subscriptions);

            reply
                .edit(
                    ctx,
                    CreateReply::new()
                        .flags(MessageFlags::IS_COMPONENTS_V2)
                        .components(components),
                )
                .await?;
        }

        Ok(())
    }
}

async fn process_subscription_batch(
    ctx: Context<'_>,
    urls: &[&str],
    subscriber: &crate::database::model::SubscriberModel,
    is_subscribe: bool,
) -> Result<(), Error> {
    let mut states: Vec<String> = vec!["⏳ Processing...".to_string(); urls.len()];
    let mut last_send = Instant::now();
    let mut reply: Option<ReplyHandle<'_>> = None;

    for (i, url) in urls.iter().enumerate() {
        let result_str = if is_subscribe {
            ctx.data()
                .service
                .feed_subscription
                .subscribe(url, subscriber)
                .await
                .map(|res| res.to_string())
        } else {
            ctx.data()
                .service
                .feed_subscription
                .unsubscribe(url, subscriber)
                .await
                .map(|res| res.to_string())
        };

        states[i] = result_str.unwrap_or_else(|e| format!("❌ {e}"));

        let is_final = i + 1 == urls.len();
        if last_send.elapsed().as_secs() > UPDATE_INTERVAL_SECS || is_final {
            let resp = SubscriptionBatchView::create(&states, is_final);
            match reply {
                None => reply = Some(ctx.send(resp).await?),
                Some(ref r) => r.edit(ctx, resp).await?,
            }
            last_send = Instant::now();
        }
    }
    Ok(())
}

pub async fn verify_server_config(
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

async fn get_subscriber(
    ctx: Context<'_>,
    send_into: &SendInto,
) -> Result<crate::database::model::SubscriberModel, Error> {
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
        f.name.insert_str(0, "(DM) ");
    }
    for f in &mut guild_feeds {
        f.name.insert_str(0, "(Server) ");
    }

    user_feeds.append(&mut guild_feeds);
    user_feeds
}

fn parse_and_validate(links: &str) -> Result<Vec<&str>, BotError> {
    let urls: Vec<&str> = links.split(',').map(|s| s.trim()).collect();
    validate(&urls)?;
    Ok(urls)
}

fn validate(urls: &[&str]) -> Result<(), BotError> {
    if urls.len() > MAX_URLS_PER_REQUEST {
        return Err(BotError::InvalidCommandArgument {
            parameter: "links".to_string(),
            reason: format!(
                "Too many links provided. Please provide no more than {} links at a time.",
                MAX_URLS_PER_REQUEST
            ),
        });
    }
    Ok(())
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

    #[test]
    fn test_validate_urls_accepts_valid_count() {
        let urls = vec!["url1", "url2", "url3"];
        assert!(validate(&urls).is_ok());
    }

    #[test]
    fn test_validate_urls_rejects_too_many() {
        let urls = vec!["url"; 11];
        let result = validate(&urls);
        assert!(result.is_err());
        match result.unwrap_err() {
            BotError::InvalidCommandArgument { parameter, reason } => {
                assert_eq!(parameter, "links");
                assert!(reason.contains("no more than 10"));
            }
            _ => panic!("Expected InvalidCommandArgument error"),
        }
    }

    #[test]
    fn test_validate_urls_accepts_exactly_ten() {
        let urls = vec!["url"; 10];
        assert!(validate(&urls).is_ok());
    }
}
