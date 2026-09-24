use std::time::Instant;

use pwr_plugin_protocol::FeedsSettings;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

use crate::FEED_UNSUBSCRIBE_COMMAND_NAME;
use crate::Platforms;
use crate::entity::SubscriberEntity;
use crate::entity::SubscriberType;
use crate::service::error::ServiceError;
use crate::service::feed_subscription::FeedSubscriptionService;
use crate::service::feed_subscription::SubscribeResult;
use crate::service::feed_subscription::SubscriberTarget;
use crate::service::feed_subscription::UnsubscribeResult;
use crate::update::feed_batch::FeedBatchModel;
use crate::update::feed_batch::FeedBatchPhase;
use crate::update::feed_list::FeedListEffect;
use crate::update::feed_list::FeedListModel;
use crate::update::feed_list::FeedListMsg;
use crate::update::feed_list::update as update_list;
use crate::view;

pub const MAX_URLS_PER_REQUEST: usize = 10;
pub const SUBSCRIPTIONS_PER_PAGE: u32 = 10;
const UPDATE_INTERVAL_SECS: u64 = 2;
const ACTOR_CONTEXT_KEY: &str = "_context";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendInto {
    Server,
    Dm,
}

impl SendInto {
    pub fn subscriber_type(self) -> SubscriberType {
        match self {
            Self::Dm => SubscriberType::Dm,
            Self::Server => SubscriberType::Guild,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("Invalid argument for {parameter}: {reason}")]
    InvalidArgument { parameter: String, reason: String },
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    #[error("You have to be in a server to use this command")]
    GuildOnly,
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("{0}")]
    Service(#[from] ServiceError),
    #[error("Invalid plugin invocation: {0}")]
    InvalidInvocation(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorContext {
    pub user_id: u64,
    #[serde(default)]
    pub member_roles: Vec<u64>,
    pub member_permissions: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedListSession {
    pub model: FeedListModel,
    pub subscriber: SubscriberEntity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedBatchSession {
    pub model: FeedBatchModel,
    pub subscriber: SubscriberEntity,
}

const ADMINISTRATOR_PERMISSION: u64 = 1 << 3;
const MANAGE_GUILD_PERMISSION: u64 = 1 << 5;

fn actor_context(args: &Value) -> Result<ActorContext, CommandError> {
    serde_json::from_value(
        args.get(ACTOR_CONTEXT_KEY)
            .cloned()
            .ok_or_else(|| CommandError::InvalidInvocation("missing actor context".into()))?,
    )
    .map_err(|error| CommandError::InvalidInvocation(error.to_string()))
}

fn send_into(args: &Value, option_name: &str) -> Result<SendInto, CommandError> {
    match args.get(option_name) {
        None => Ok(SendInto::Dm),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("dm") => Ok(SendInto::Dm),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("server") => Ok(SendInto::Server),
        Some(Value::String(value)) => Err(CommandError::InvalidArgument {
            parameter: "send_into".into(),
            reason: format!("unknown send_into value `{value}`"),
        }),
        Some(_) => Err(CommandError::InvalidArgument {
            parameter: "send_into".into(),
            reason: "must be a string".into(),
        }),
    }
}

pub fn parse_and_validate_urls(links: &str) -> Result<Vec<String>, CommandError> {
    let urls = links
        .split(',')
        .map(str::trim)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if urls.len() > MAX_URLS_PER_REQUEST {
        return Err(CommandError::InvalidArgument {
            parameter: "links".into(),
            reason: format!(
                concat!("Too many links provided. Please provide no more than {} links at a time."),
                MAX_URLS_PER_REQUEST
            ),
        });
    }
    Ok(urls)
}

fn target_id(
    send_into: SendInto,
    guild_id: Option<u64>,
    user_id: u64,
) -> Result<String, CommandError> {
    match send_into {
        SendInto::Dm => Ok(user_id.to_string()),
        SendInto::Server => guild_id
            .map(|guild_id| guild_id.to_string())
            .ok_or_else(|| CommandError::InvalidArgument {
                parameter: "Server".into(),
                reason: "You have to be in a server to do this command with send_into: server"
                    .into(),
            }),
    }
}

async fn subscriber_for(
    service: &FeedSubscriptionService,
    args: &Value,
    option_name: &str,
) -> Result<(SendInto, SubscriberEntity, ActorContext), CommandError> {
    let context = actor_context(args)?;
    let send_into = send_into(args, option_name)?;
    let guild_id = args.get("guild_id").and_then(Value::as_u64);
    let target = SubscriberTarget {
        subscriber_type: send_into.subscriber_type(),
        target_id: target_id(send_into, guild_id, context.user_id)?,
    };
    let subscriber = service.get_or_create_subscriber(&target).await?;
    Ok((send_into, subscriber, context))
}

async fn verify_server_config(
    service: &FeedSubscriptionService,
    send_into: SendInto,
    is_subscribe: bool,
    context: &ActorContext,
    guild_id: Option<u64>,
) -> Result<(), CommandError> {
    if send_into != SendInto::Server {
        return Ok(());
    }
    let guild_id = guild_id.ok_or(CommandError::GuildOnly)?;
    let settings = service.get_feed_settings(guild_id).await?;
    if settings.channel_id.is_none() {
        return Err(CommandError::ConfigurationError(
            concat!(
                "Server feed settings are not configured. A server admin must run `/settings` ",
                "to configure a notification channel first."
            )
            .into(),
        ));
    }
    verify_configured_role(&settings, is_subscribe, context)
}

fn verify_configured_role(
    settings: &FeedsSettings,
    is_subscribe: bool,
    context: &ActorContext,
) -> Result<(), CommandError> {
    let role_id = if is_subscribe {
        settings.subscribe_role_id.as_deref()
    } else {
        settings.unsubscribe_role_id.as_deref()
    };
    let Some(role_id) = role_id else {
        return Ok(());
    };
    let role_id = role_id
        .parse::<u64>()
        .map_err(|error| CommandError::InvalidArgument {
            parameter: "role".into(),
            reason: format!("invalid configured role id `{role_id}`: {error}"),
        })?;
    if !context.member_roles.contains(&role_id) {
        return Err(CommandError::PermissionDenied(format!(
            "You need the <@&{role_id}> role to perform this action."
        )));
    }
    Ok(())
}

fn subscribe_result(result: SubscribeResult) -> String {
    match result {
        SubscribeResult::Success { feed } => format!(
            "✅ **Successfully** subscribed to [{}](<{}>)",
            feed.name, feed.source_url
        ),
        SubscribeResult::AlreadySubscribed { feed } => format!(
            "❌ You are **already subscribed** to [{}](<{}>)",
            feed.name, feed.source_url
        ),
    }
}

fn unsubscribe_result(result: UnsubscribeResult) -> String {
    match result {
        UnsubscribeResult::Success { feed } => format!(
            "✅ **Successfully** unsubscribed from [{}](<{}>)",
            feed.name, feed.source_url
        ),
        UnsubscribeResult::AlreadyUnsubscribed { feed } => format!(
            "❌ You are **not subscribed** to [{}](<{}>)",
            feed.name, feed.source_url
        ),
        UnsubscribeResult::NoneSubscribed { url } => {
            format!("❌ You are **not subscribed** to <{url}>")
        }
    }
}

fn list_envelope(session: &FeedListSession) -> Value {
    json!({
        "data": view::message_data(view::feed_list::components(&session.model)),
        "ephemeral": false,
        "view": session,
    })
}

fn batch_envelope(session: &FeedBatchSession) -> Value {
    json!({
        "data": view::message_data(view::feed_batch::components(&session.model)),
        "ephemeral": false,
        "view": session,
    })
}

async fn list_for_subscriber(
    service: &FeedSubscriptionService,
    subscriber: SubscriberEntity,
) -> Result<Value, CommandError> {
    let subscriptions = service
        .list_paginated_subscriptions(&subscriber, 1u32, SUBSCRIPTIONS_PER_PAGE)
        .await?;
    let session = FeedListSession {
        model: FeedListModel::new(subscriptions, SUBSCRIPTIONS_PER_PAGE),
        subscriber,
    };
    Ok(list_envelope(&session))
}

pub async fn invoke_list(
    service: &FeedSubscriptionService,
    args: &Value,
) -> Result<Value, CommandError> {
    let (_, subscriber, _) = subscriber_for(service, args, "sent_into").await?;
    list_for_subscriber(service, subscriber).await
}

pub(crate) fn translate_list_action(
    custom_id: &str,
    model: &FeedListModel,
) -> Result<FeedListMsg, CommandError> {
    if custom_id == view::feed_list::EDIT {
        Ok(FeedListMsg::Edit)
    } else if custom_id == view::feed_list::VIEW {
        Ok(FeedListMsg::View)
    } else if custom_id == view::feed_list::SAVE {
        Ok(FeedListMsg::Save)
    } else if let Some(index) = custom_id
        .strip_prefix(view::feed_list::UNSUBSCRIBE_PREFIX)
        .and_then(|value| value.split(':').next())
        .and_then(|value| value.parse::<usize>().ok())
    {
        let source_url = model
            .subscriptions
            .get(index)
            .ok_or_else(|| {
                CommandError::InvalidInvocation("subscription index out of range".into())
            })?
            .feed
            .source_url
            .clone();
        Ok(FeedListMsg::ToggleUnsub { source_url })
    } else if custom_id == view::feed_list::FIRST {
        Ok(FeedListMsg::Pagination(
            pwr_plugin_support::pagination::PaginationAction::First,
        ))
    } else if custom_id == view::feed_list::PREVIOUS {
        Ok(FeedListMsg::Pagination(
            pwr_plugin_support::pagination::PaginationAction::Prev,
        ))
    } else if custom_id == view::feed_list::NEXT {
        Ok(FeedListMsg::Pagination(
            pwr_plugin_support::pagination::PaginationAction::Next,
        ))
    } else if custom_id == view::feed_list::LAST {
        Ok(FeedListMsg::Pagination(
            pwr_plugin_support::pagination::PaginationAction::Last,
        ))
    } else {
        Err(CommandError::InvalidInvocation(format!(
            "unknown feed list action `{custom_id}`"
        )))
    }
}

pub async fn interact_list(
    service: &FeedSubscriptionService,
    args: &Value,
) -> Result<Value, CommandError> {
    let mut session: FeedListSession = serde_json::from_value(
        args.get("view")
            .cloned()
            .ok_or_else(|| CommandError::InvalidInvocation("missing list view state".into()))?,
    )
    .map_err(|error| CommandError::InvalidInvocation(error.to_string()))?;
    let custom_id = args
        .get("custom_id")
        .and_then(Value::as_str)
        .ok_or_else(|| CommandError::InvalidInvocation("missing custom_id".into()))?;
    let message = translate_list_action(custom_id, &session.model)?;

    let mut effects = update_list(message, &mut session.model);
    while let Some(effect) = effects.pop() {
        match effect {
            FeedListEffect::QuerySubscriptions { page, per_page } => {
                let subscriptions = service
                    .list_paginated_subscriptions(&session.subscriber, page, per_page)
                    .await?;
                effects.extend(update_list(
                    FeedListMsg::SubscriptionsLoaded(subscriptions),
                    &mut session.model,
                ));
            }
            FeedListEffect::SaveUnsubscribes(urls) => {
                for url in urls {
                    if let Err(error) = service.unsubscribe(&url, &session.subscriber).await {
                        eprintln!("feed unsubscribe failed for {url}: {error}");
                    }
                }
                effects.extend(update_list(FeedListMsg::Saved, &mut session.model));
            }
        }
    }
    Ok(list_envelope(&session))
}

pub async fn invoke_batch<F>(
    service: &FeedSubscriptionService,
    args: &Value,
    is_subscribe: bool,
    mut on_progress: F,
) -> Result<Value, CommandError>
where
    F: FnMut(Value),
{
    let context = actor_context(args)?;
    let send_into = send_into(args, "send_into")?;
    let links =
        args.get("links")
            .and_then(Value::as_str)
            .ok_or_else(|| CommandError::InvalidArgument {
                parameter: "links".into(),
                reason: "missing links".into(),
            })?;
    let urls = parse_and_validate_urls(links)?;
    let guild_id = args.get("guild_id").and_then(Value::as_u64);
    verify_server_config(service, send_into, is_subscribe, &context, guild_id).await?;
    let (_, subscriber, _) = subscriber_for(service, args, "send_into").await?;
    let mut states = vec!["⏳ Processing...".to_string(); urls.len()];
    let mut last_send = Instant::now();
    let mut session = FeedBatchSession {
        model: FeedBatchModel::new(states.clone(), FeedBatchPhase::Confirm, subscriber.r#type),
        subscriber,
    };
    on_progress(batch_envelope(&session));

    for (index, url) in urls.iter().enumerate() {
        let result = if is_subscribe {
            service
                .subscribe(url, &session.subscriber)
                .await
                .map(subscribe_result)
        } else {
            service
                .unsubscribe(url, &session.subscriber)
                .await
                .map(unsubscribe_result)
        };
        states[index] = result.unwrap_or_else(|error| format!("❌ {error}"));
        let is_final = index + 1 == urls.len();
        if last_send.elapsed().as_secs() > UPDATE_INTERVAL_SECS || is_final {
            session.model = FeedBatchModel::new(
                states.clone(),
                if is_final {
                    FeedBatchPhase::Done
                } else {
                    FeedBatchPhase::Confirm
                },
                session.subscriber.r#type,
            );
            on_progress(batch_envelope(&session));
            last_send = Instant::now();
        }
    }
    Ok(batch_envelope(&session))
}

pub async fn interact_batch(
    service: &FeedSubscriptionService,
    args: &Value,
) -> Result<Value, CommandError> {
    let session: FeedBatchSession = serde_json::from_value(
        args.get("view")
            .cloned()
            .ok_or_else(|| CommandError::InvalidInvocation("missing batch view state".into()))?,
    )
    .map_err(|error| CommandError::InvalidInvocation(error.to_string()))?;
    let custom_id = args
        .get("custom_id")
        .and_then(Value::as_str)
        .ok_or_else(|| CommandError::InvalidInvocation("missing custom_id".into()))?;
    if custom_id == view::feed_batch::VIEW_SUBSCRIPTIONS {
        return list_for_subscriber(service, session.subscriber).await;
    }
    Err(CommandError::InvalidInvocation(format!(
        "unknown feed batch action `{custom_id}`"
    )))
}

pub async fn interact_feed_view(
    service: &FeedSubscriptionService,
    args: &Value,
) -> Result<Value, CommandError> {
    let custom_id = args
        .get("custom_id")
        .and_then(Value::as_str)
        .ok_or_else(|| CommandError::InvalidInvocation("missing custom_id".into()))?;
    if custom_id.starts_with("feed-batch:") {
        return interact_batch(service, args).await;
    }
    if custom_id.starts_with("feed-list:") {
        return interact_list(service, args).await;
    }
    Err(CommandError::InvalidInvocation(format!(
        "unknown feed view action `{custom_id}`"
    )))
}

pub async fn autocomplete(
    service: &FeedSubscriptionService,
    platforms: &Platforms,
    command_name: &str,
    args: &Value,
) -> Value {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if args.get("option").and_then(Value::as_str) != Some("links") {
        return json!({ "choices": [] });
    }
    if command_name == FEED_UNSUBSCRIBE_COMMAND_NAME {
        if query.trim().is_empty() {
            return json!({
                "choices": [{
                    "name": "Start typing to see suggestions",
                    "value": "Start typing to see suggestions",
                }]
            });
        }
        let Some(context) = args
            .get(ACTOR_CONTEXT_KEY)
            .and_then(|value| serde_json::from_value::<ActorContext>(value.clone()).ok())
        else {
            return json!({ "choices": [] });
        };
        let guild_id = args.get("guild_id").and_then(Value::as_u64);
        let (user, guild) = service
            .get_both_subscribers(
                context.user_id.to_string(),
                guild_id.map(|guild_id| guild_id.to_string()),
            )
            .await;
        if user.is_none() && guild.is_none() {
            return json!({ "choices": [] });
        }
        let feeds = service.search_and_combine_feeds(&query, user, guild).await;
        if guild_id.is_none() && feeds.is_empty() {
            return json!({
                "choices": [{
                    "name": concat!(
                        "You have no subscriptions yet. Subscribe first with ",
                        "`/subscribe` command"
                    ),
                    "value": concat!(
                        "You have no subscriptions yet. Subscribe first with ",
                        "`/subscribe` command"
                    ),
                }]
            });
        }
        let choices = feeds
            .into_iter()
            .take(25)
            .map(|feed| json!({ "name": feed.name, "value": feed.source_url }))
            .collect::<Vec<_>>();
        return json!({ "choices": choices });
    }

    let lower_query = query.to_lowercase();
    let mut choices = vec![json!({ "name": "Supported feeds are:", "value": "foo" })];
    choices.extend(
        platforms
            .get_all_platforms()
            .into_iter()
            .filter(|platform| {
                let info = &platform.get_base().info;
                query.is_empty()
                    || info.name.to_lowercase().contains(&lower_query)
                    || info.api_domain.contains(&lower_query)
            })
            .map(|platform| {
                let info = &platform.get_base().info;
                json!({
                    "name": format!("{} ({})", info.name, info.api_domain),
                    "value": info.api_domain,
                })
            })
            .take(24),
    );
    json!({ "choices": choices })
}

/// Requires a guild administrator for a settings command invocation.
pub fn verify_settings_invocation(args: &Value) -> Result<(), CommandError> {
    let context = actor_context(args)?;
    verify_settings_permissions(&context)
}

/// Requires a guild administrator for a settings component interaction.
pub fn verify_settings_interaction(args: &Value) -> Result<(), CommandError> {
    verify_settings_invocation(args)
}

fn verify_settings_permissions(context: &ActorContext) -> Result<(), CommandError> {
    let permissions = context.member_permissions.unwrap_or_default();
    if permissions & (ADMINISTRATOR_PERMISSION | MANAGE_GUILD_PERMISSION) == 0 {
        return Err(CommandError::PermissionDenied(
            "You need Manage Server or Administrator permission to configure feed settings.".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_target_maps_to_subscriber_type() {
        assert_eq!(SendInto::Dm.subscriber_type(), SubscriberType::Dm);
        assert_eq!(SendInto::Server.subscriber_type(), SubscriberType::Guild);
    }

    #[test]
    fn dm_target_uses_the_user_id() {
        assert_eq!(target_id(SendInto::Dm, Some(999), 12345).unwrap(), "12345");
    }

    #[test]
    fn server_target_uses_the_guild_id() {
        assert_eq!(
            target_id(SendInto::Server, Some(999), 12345).unwrap(),
            "999"
        );
    }

    #[test]
    fn server_target_requires_a_guild() {
        let error = target_id(SendInto::Server, None, 12345).unwrap_err();
        assert!(error.to_string().contains("send_into: server"));
    }

    #[test]
    fn url_parser_splits_and_trims() {
        assert_eq!(
            parse_and_validate_urls("url1, url2 ,url3").unwrap(),
            ["url1", "url2", "url3"]
        );
    }

    #[test]
    fn url_parser_accepts_ten_links() {
        let links = ["url"; 10].join(",");
        assert_eq!(parse_and_validate_urls(&links).unwrap().len(), 10);
    }

    #[test]
    fn url_parser_rejects_eleven_links() {
        let links = ["url"; 11].join(",");
        let error = parse_and_validate_urls(&links).unwrap_err();
        assert!(error.to_string().contains("no more than 10"));
    }

    #[test]
    fn configured_role_is_required() {
        let settings = FeedsSettings {
            subscribe_role_id: Some("123".into()),
            ..FeedsSettings::default()
        };
        let context = ActorContext {
            user_id: 42,
            member_roles: vec![123],
            member_permissions: None,
        };
        assert!(verify_configured_role(&settings, true, &context).is_ok());
    }

    #[test]
    fn settings_permission_accepts_manage_guild() {
        let context = ActorContext {
            user_id: 42,
            member_roles: vec![],
            member_permissions: Some(1 << 5),
        };

        assert!(verify_settings_permissions(&context).is_ok());
    }

    #[test]
    fn settings_permission_rejects_a_user_without_server_management() {
        let context = ActorContext {
            user_id: 42,
            member_roles: vec![],
            member_permissions: Some(1 << 1),
        };

        assert!(verify_settings_permissions(&context).is_err());
    }

    #[test]
    fn settings_invocation_rejects_non_admin_and_accepts_admin_actors() {
        let non_admin = json!({
            "_context": {
                "user_id": 42,
                "member_roles": [],
                "member_permissions": 0,
            },
        });
        let admin = json!({
            "_context": {
                "user_id": 42,
                "member_roles": [],
                "member_permissions": 1 << 5,
            },
        });

        assert!(verify_settings_invocation(&non_admin).is_err());
        assert!(verify_settings_invocation(&admin).is_ok());
    }

    #[test]
    fn settings_persistence_uses_the_same_admin_actor_policy() {
        let non_admin = json!({
            "_context": {
                "user_id": 42,
                "member_roles": [],
                "member_permissions": 0,
            },
            "custom_id": "feeds:toggle",
        });
        let administrator = json!({
            "_context": {
                "user_id": 42,
                "member_roles": [],
                "member_permissions": 1 << 3,
            },
            "custom_id": "feeds:toggle",
        });

        assert!(verify_settings_interaction(&non_admin).is_err());
        assert!(verify_settings_interaction(&administrator).is_ok());
    }

    #[test]
    fn missing_configured_role_is_rejected() {
        let settings = FeedsSettings {
            unsubscribe_role_id: Some("123".into()),
            ..FeedsSettings::default()
        };
        let context = ActorContext {
            user_id: 42,
            member_roles: vec![456],
            member_permissions: None,
        };
        assert!(verify_configured_role(&settings, false, &context).is_err());
    }
}
