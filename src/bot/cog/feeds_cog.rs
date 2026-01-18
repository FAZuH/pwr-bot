/// Cog to manage feed subscriptions
use std::fmt::Display;
use std::str::FromStr;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use poise::ChoiceParameter;
use poise::CreateReply;
use poise::ReplyHandle;
use poise::serenity_prelude::AutocompleteChoice;
use poise::serenity_prelude::CreateAutocompleteResponse;
use serenity::all::ButtonStyle;
use serenity::all::ChannelId;
use serenity::all::ChannelType;
use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionCollector;
use serenity::all::ComponentInteractionDataKind;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateInteractionResponse;
use serenity::all::CreateInteractionResponseMessage;
use serenity::all::CreateSection;
use serenity::all::CreateSectionAccessory;
use serenity::all::CreateSectionComponent;
use serenity::all::CreateSelectMenu;
use serenity::all::CreateSelectMenuKind;
use serenity::all::CreateSelectMenuOption;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateThumbnail;
use serenity::all::CreateUnfurledMediaItem;
use serenity::all::GenericChannelId;
use serenity::all::GuildId;
use serenity::all::MessageFlags;
use serenity::all::RoleId;
use serenity::all::UserId;
use serenity::futures::StreamExt;

use crate::bot::checks::check_guild_permissions;
use crate::bot::cog::Context;
use crate::bot::cog::Error;
use crate::bot::components::PageNavigationComponent;
use crate::bot::components::Pagination;
use crate::bot::error::BotError;
use crate::database::model::ServerSettings;
use crate::database::model::SubscriberModel;
use crate::database::model::SubscriberType;
use crate::service::feed_subscription_service::SubscribeResult;
use crate::service::feed_subscription_service::SubscriberTarget;
use crate::service::feed_subscription_service::SubscriptionInfo;
use crate::service::feed_subscription_service::UnsubscribeResult;

const MAX_URLS_PER_REQUEST: usize = 10;
const ITEMS_PER_PAGE: u32 = 10;
const UPDATE_INTERVAL_SECS: u64 = 2;
const INTERACTION_TIMEOUT_SECS: u64 = 120;

// ============================================================================
// Types and Display Implementations
// ============================================================================

#[derive(ChoiceParameter)]
enum SendInto {
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

impl Display for SendInto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DM => write!(f, "dm"),
            Self::Server => write!(f, "server"),
        }
    }
}

impl Display for SubscribeResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            SubscribeResult::Success { feed } => format!(
                "✅ **Successfully** subscribed to [{}](<{}>)",
                feed.name, feed.source_url
            ),
            SubscribeResult::AlreadySubscribed { feed } => format!(
                "❌ You are **already subscribed** to [{}](<{}>)",
                feed.name, feed.source_url
            ),
        };
        write!(f, "{}", msg)
    }
}

impl Display for UnsubscribeResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            UnsubscribeResult::Success { feed } => format!(
                "✅ **Successfully** unsubscribed from [{}](<{}>)",
                feed.name, feed.source_url
            ),
            UnsubscribeResult::AlreadyUnsubscribed { feed } => format!(
                "❌ You are **not subscribed** to [{}](<{}>)",
                feed.name, feed.source_url
            ),
            UnsubscribeResult::NoneSubscribed { url } => {
                format!("❌ You are **not subscribed** to <{}>", url)
            }
        };
        write!(f, "{}", msg)
    }
}

// ============================================================================
// Main Cog
// ============================================================================

pub struct FeedsCog;

impl FeedsCog {
    // ------------------------------------------------------------------------
    // Commands
    // ------------------------------------------------------------------------

    #[poise::command(
        slash_command,
        guild_only,
        default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
    )]
    pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
        ctx.defer().await?;
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();

        let mut settings = ctx.data().service.get_server_settings(guild_id).await?;

        let msg_handle = ctx
            .send(
                CreateReply::new()
                    .flags(MessageFlags::IS_COMPONENTS_V2)
                    .components(ServerSettingsPage::create(&settings)),
            )
            .await?;

        FeedsCog::handle_settings_interactions(ctx, msg_handle, &mut settings, guild_id).await?;
        Ok(())
    }

    #[poise::command(slash_command)]
    pub async fn subscribe(
        ctx: Context<'_>,
        #[description = "Link(s) of the feeds. Separate links with commas (,)"]
        #[autocomplete = "FeedsCog::autocomplete_supported_feeds"]
        links: String,
        #[description = "Where to send the notifications. Default to your DM"] send_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        ctx.defer().await?;

        let send_into = send_into.unwrap_or(SendInto::DM);
        let urls = FeedsCog::parse_and_validate_urls(&links)?;

        FeedsCog::verify_server_config(ctx, &send_into, true).await?;

        let subscriber = FeedsCog::get_subscriber(ctx, &send_into).await?;
        FeedsCog::process_subscription_batch(ctx, &urls, &subscriber, true).await?;

        Ok(())
    }

    #[poise::command(slash_command)]
    pub async fn unsubscribe(
        ctx: Context<'_>,
        #[description = "Link(s) of the feeds. Separate links with commas (,)"]
        #[autocomplete = "FeedsCog::autocomplete_subscriptions"]
        links: String,
        #[description = "Where notifications were being sent. Default to DM"] send_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        ctx.defer().await?;

        let send_into = send_into.unwrap_or(SendInto::DM);
        let urls = FeedsCog::parse_and_validate_urls(&links)?;

        FeedsCog::verify_server_config(ctx, &send_into, false).await?;

        let subscriber = FeedsCog::get_subscriber(ctx, &send_into).await?;
        FeedsCog::process_subscription_batch(ctx, &urls, &subscriber, false).await?;

        Ok(())
    }

    #[poise::command(slash_command)]
    pub async fn subscriptions(
        ctx: Context<'_>,
        #[description = "Where the notifications are being sent. Default to DM"] sent_into: Option<
            SendInto,
        >,
    ) -> Result<(), Error> {
        ctx.defer().await?;
        let sent_into = sent_into.unwrap_or(SendInto::DM);

        let subscriber = FeedsCog::get_subscriber(ctx, &sent_into).await?;
        let total_items = ctx
            .data()
            .service
            .get_subscription_count(&subscriber)
            .await?;

        FeedsCog::show_paginated_subscriptions(ctx, &subscriber, total_items).await?;
        Ok(())
    }

    // ------------------------------------------------------------------------
    // Settings UI
    // ------------------------------------------------------------------------

    async fn handle_settings_interactions(
        ctx: Context<'_>,
        msg_handle: ReplyHandle<'_>,
        settings: &mut ServerSettings,
        guild_id: u64,
    ) -> Result<(), Error> {
        let msg = msg_handle.message().await?.into_owned();
        let author_id = ctx.author().id;

        let mut collector = ComponentInteractionCollector::new(ctx.serenity_context())
            .message_id(msg.id)
            .author_id(author_id)
            .timeout(Duration::from_secs(INTERACTION_TIMEOUT_SECS))
            .stream();

        while let Some(interaction) = collector.next().await {
            if FeedsCog::update_setting_from_interaction(settings, &interaction) {
                ctx.data()
                    .service
                    .update_server_settings(guild_id, settings.clone())
                    .await?;
            }

            interaction
                .create_response(
                    ctx.http(),
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .components(ServerSettingsPage::create(settings)),
                    ),
                )
                .await?;
        }

        Ok(())
    }

    fn update_setting_from_interaction(
        settings: &mut ServerSettings,
        interaction: &ComponentInteraction,
    ) -> bool {
        match &interaction.data.kind {
            ComponentInteractionDataKind::StringSelect { values }
                if interaction.data.custom_id == "server_settings_enabled" =>
            {
                if let Some(value) = values.first() {
                    settings.enabled = Some(value == "true");
                    return true;
                }
            }
            ComponentInteractionDataKind::ChannelSelect { values }
                if interaction.data.custom_id == "server_settings_channel" =>
            {
                settings.channel_id = values.first().map(|id| id.to_string());
                return true;
            }
            ComponentInteractionDataKind::RoleSelect { values }
                if interaction.data.custom_id == "server_settings_sub_role" =>
            {
                settings.subscribe_role_id = if values.is_empty() {
                    None
                } else {
                    values.first().map(|id| id.to_string())
                };
                return true;
            }
            ComponentInteractionDataKind::RoleSelect { values }
                if interaction.data.custom_id == "server_settings_unsub_role" =>
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

    // ------------------------------------------------------------------------
    // Subscription Processing
    // ------------------------------------------------------------------------

    async fn process_subscription_batch(
        ctx: Context<'_>,
        urls: &[&str],
        subscriber: &SubscriberModel,
        is_subscribe: bool,
    ) -> Result<(), Error> {
        let mut states: Vec<String> = vec!["⏳ Processing...".to_string(); urls.len()];
        let interval = Duration::from_secs(UPDATE_INTERVAL_SECS);
        let mut last_send = Instant::now();
        let mut reply: Option<ReplyHandle<'_>> = None;

        for (i, url) in urls.iter().enumerate() {
            let result_str = if is_subscribe {
                ctx.data()
                    .service
                    .subscribe(url, subscriber)
                    .await
                    .map(|res| res.to_string())
            } else {
                ctx.data()
                    .service
                    .unsubscribe(url, subscriber)
                    .await
                    .map(|res| res.to_string())
            };

            states[i] = result_str.unwrap_or_else(|e| format!("❌ {e}"));

            let is_final = i + 1 == urls.len();
            if last_send.elapsed() > interval || is_final {
                let resp = SubscriptionBatchPage::create(&states, is_final);
                match reply {
                    None => reply = Some(ctx.send(resp).await?),
                    Some(ref r) => r.edit(ctx, resp).await?,
                }
                last_send = Instant::now();
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------------
    // Subscriptions List
    // ------------------------------------------------------------------------

    async fn show_paginated_subscriptions(
        ctx: Context<'_>,
        subscriber: &SubscriberModel,
        total_items: u32,
    ) -> Result<(), Error> {
        let pages = total_items.div_ceil(ITEMS_PER_PAGE);
        let mut navigation =
            PageNavigationComponent::new(&ctx, Pagination::new(pages, ITEMS_PER_PAGE, 1));

        let reply = ctx
            .send(
                CreateReply::new()
                    .flags(MessageFlags::IS_COMPONENTS_V2)
                    .components(
                        FeedsCog::create_subscription_page(&ctx, subscriber, &navigation).await?,
                    ),
            )
            .await?;

        while navigation.listen(Duration::from_secs(60)).await {
            reply
                .edit(
                    ctx,
                    CreateReply::new()
                        .flags(MessageFlags::IS_COMPONENTS_V2)
                        .components(
                            FeedsCog::create_subscription_page(&ctx, subscriber, &navigation)
                                .await?,
                        ),
                )
                .await?;
        }

        Ok(())
    }

    async fn create_subscription_page<'a>(
        ctx: &Context<'_>,
        subscriber: &SubscriberModel,
        navigation: &'a PageNavigationComponent<'_>,
    ) -> Result<Vec<CreateComponent<'a>>, Error> {
        let subscriptions = ctx
            .data()
            .service
            .list_paginated_subscriptions(
                subscriber,
                navigation.pagination.current_page,
                navigation.pagination.per_page,
            )
            .await?;

        if subscriptions.is_empty() {
            return Ok(SubscriptionsListPage::create_empty());
        }

        Ok(SubscriptionsListPage::create(subscriptions, navigation))
    }

    // ------------------------------------------------------------------------
    // Autocomplete
    // ------------------------------------------------------------------------

    async fn autocomplete_subscriptions<'a>(
        ctx: Context<'_>,
        partial: &str,
    ) -> CreateAutocompleteResponse<'a> {
        if partial.trim().is_empty() {
            return CreateAutocompleteResponse::new().set_choices(vec![AutocompleteChoice::from(
                "Start typing to see suggestions",
            )]);
        }

        let (user_subscriber, guild_subscriber) = FeedsCog::get_both_subscribers(ctx).await;

        if user_subscriber.is_none() && guild_subscriber.is_none() {
            return CreateAutocompleteResponse::new();
        }

        let feeds =
            FeedsCog::search_and_combine_feeds(ctx, partial, user_subscriber, guild_subscriber)
                .await;

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

    async fn autocomplete_supported_feeds<'a>(
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

    // ------------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------------

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
                    .get_or_create_subscriber(&guild_target)
                    .await
                    .ok()
            }
            None => None,
        };

        (user_subscriber, guild_subscriber)
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
                .search_subcriptions(&sub, partial)
                .await
                .unwrap_or_default(),
            None => vec![],
        };

        let mut guild_feeds = match guild_subscriber {
            Some(sub) => ctx
                .data()
                .service
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

            check_guild_permissions(ctx, role_id).await?;
        }
        Ok(())
    }

    async fn get_subscriber(
        ctx: Context<'_>,
        send_into: &SendInto,
    ) -> Result<SubscriberModel, Error> {
        let target_id = FeedsCog::get_target_id(ctx, send_into)?;
        let subscriber_type = SubscriberType::from(send_into);
        let target = SubscriberTarget {
            subscriber_type,
            target_id,
        };
        Ok(ctx.data().service.get_or_create_subscriber(&target).await?)
    }

    fn parse_and_validate_urls(links: &str) -> Result<Vec<&str>, BotError> {
        let urls: Vec<&str> = links.split(',').map(|s| s.trim()).collect();
        FeedsCog::validate_urls(&urls)?;
        Ok(urls)
    }

    fn validate_urls(urls: &[&str]) -> Result<(), BotError> {
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

    fn get_target_id(ctx: Context<'_>, send_into: &SendInto) -> Result<String, BotError> {
        FeedsCog::get_target_id_inner(ctx.guild_id(), ctx.author().id, send_into)
    }

    fn get_target_id_inner(
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
}

// ============================================================================
// Page Components
// ============================================================================

struct SubscriptionBatchPage;

impl SubscriptionBatchPage {
    fn create(states: &[String], is_final: bool) -> CreateReply<'_> {
        let text_components: Vec<CreateContainerComponent> = states
            .iter()
            .map(|s| CreateContainerComponent::TextDisplay(CreateTextDisplay::new(s.clone())))
            .collect();

        let mut components = vec![CreateComponent::Container(CreateContainer::new(
            text_components,
        ))];

        if is_final {
            let nav_button = CreateButton::new("view_subscriptions")
                .label("View Subscriptions")
                .style(ButtonStyle::Secondary);

            components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
                vec![nav_button].into(),
            )));
        }

        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(components)
    }
}

struct SubscriptionsListPage;

impl SubscriptionsListPage {
    fn create_empty<'a>() -> Vec<CreateComponent<'a>> {
        vec![CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                "You have no subscriptions.",
            )),
        ]))]
    }

    fn create<'a>(
        subscriptions: Vec<SubscriptionInfo>,
        pagination: &'a PageNavigationComponent,
    ) -> Vec<CreateComponent<'a>> {
        let sections: Vec<CreateContainerComponent> = subscriptions
            .into_iter()
            .map(Self::create_subscription_section)
            .collect();

        let container = CreateComponent::Container(CreateContainer::new(sections));

        if pagination.pagination.pages == 1 {
            vec![container]
        } else {
            vec![container, pagination.create_buttons()]
        }
    }

    fn create_subscription_section<'a>(sub: SubscriptionInfo) -> CreateContainerComponent<'a> {
        let text = if let Some(latest) = sub.feed_latest {
            CreateTextDisplay::new(format!(
                "### {}\n\n- **Last version**: {}\n- **Last updated**: <t:{}>\n- **Source**: <{}>",
                sub.feed.name,
                latest.description,
                latest.published.timestamp(),
                sub.feed.source_url
            ))
        } else {
            CreateTextDisplay::new(format!(
                "### {}\n\n> No latest version found.\n- **Source**: <{}>",
                sub.feed.name, sub.feed.source_url
            ))
        };

        let thumbnail = CreateThumbnail::new(CreateUnfurledMediaItem::new(sub.feed.cover_url));

        CreateContainerComponent::Section(CreateSection::new(
            vec![CreateSectionComponent::TextDisplay(text)],
            CreateSectionAccessory::Thumbnail(thumbnail),
        ))
    }
}

struct ServerSettingsPage;

impl ServerSettingsPage {
    fn create(settings: &ServerSettings) -> Vec<CreateComponent<'_>> {
        let is_enabled = settings.enabled.unwrap_or(true);

        let status_text = format!(
            "## Server Feed Settings\n\n> 🛈  {}",
            if is_enabled {
                format!(
                    "Feed notifications are currently active. Notifications will be sent to <#{}>",
                    settings.channel_id.as_deref().unwrap_or("Unknown")
                )
            } else {
                "Feed notifications are currently paused. No notifications will be sent until re-enabled.".to_string()
            }
        );

        let enabled_select = CreateSelectMenu::new(
            "server_settings_enabled",
            CreateSelectMenuKind::String {
                options: vec![
                    CreateSelectMenuOption::new("🟢 Enabled", "true").default_selection(is_enabled),
                    CreateSelectMenuOption::new("🔴 Disabled", "false")
                        .default_selection(!is_enabled),
                ]
                .into(),
            },
        )
        .placeholder("Toggle feed notifications");

        let channel_text =
            "### Notification Channel\n\n> 🛈  Choose where feed updates will be posted.";

        let channel_select = CreateSelectMenu::new(
            "server_settings_channel",
            CreateSelectMenuKind::Channel {
                channel_types: Some(vec![ChannelType::Text, ChannelType::News].into()),
                default_channels: Some(Self::parse_channel_id(&settings.channel_id).into()),
            },
        )
        .placeholder(if settings.channel_id.is_some() {
            "Change notification channel"
        } else {
            "⚠️ Required: Select a notification channel"
        });

        let sub_role_text = "### Subscribe Permission\n\n> 🛈  Who can add new feeds to this server. Leave empty to allow users with \"Manage Server\" permission.";
        let sub_role_select = CreateSelectMenu::new(
            "server_settings_sub_role",
            CreateSelectMenuKind::Role {
                default_roles: Some(Self::parse_role_id(&settings.subscribe_role_id).into()),
            },
        )
        .min_values(0)
        .placeholder(if settings.subscribe_role_id.is_some() {
            "Change subscribe role"
        } else {
            "Optional: Select role for subscribe permission"
        });

        let unsub_role_text = "### Unsubscribe Permission\n\n> 🛈  Who can remove feeds from this server. Leave empty to allow users with \"Manage Server\" permission.";
        let unsub_role_select = CreateSelectMenu::new(
            "server_settings_unsub_role",
            CreateSelectMenuKind::Role {
                default_roles: Some(Self::parse_role_id(&settings.unsubscribe_role_id).into()),
            },
        )
        .min_values(0)
        .placeholder(if settings.unsubscribe_role_id.is_some() {
            "Change unsubscribe role"
        } else {
            "Optional: Select role for unsubscribe permission"
        });

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(enabled_select)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(channel_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(channel_select)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(sub_role_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(sub_role_select)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(unsub_role_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(unsub_role_select)),
        ]));

        vec![container]
    }

    fn parse_role_id(id: &Option<String>) -> Vec<RoleId> {
        id.as_ref()
            .and_then(|id| RoleId::from_str(id).ok())
            .into_iter()
            .collect()
    }

    fn parse_channel_id(id: &Option<String>) -> Vec<GenericChannelId> {
        id.as_ref()
            .and_then(|id| ChannelId::from_str(id).ok().map(GenericChannelId::from))
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_target_id_dm_returns_author_id() {
        let result = FeedsCog::get_target_id_inner(
            Some(GuildId::new(999)),
            UserId::new(12345),
            &SendInto::DM,
        );
        assert_eq!(result.unwrap(), "12345");
    }

    #[test]
    fn test_get_target_id_server_returns_guild_id() {
        let result = FeedsCog::get_target_id_inner(
            Some(GuildId::new(999)),
            UserId::new(12345),
            &SendInto::Server,
        );
        assert_eq!(result.unwrap(), "999");
    }

    #[test]
    fn test_get_target_id_server_without_guild_fails() {
        let result = FeedsCog::get_target_id_inner(None, UserId::new(12345), &SendInto::Server);
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
    fn test_validate_urls_accepts_valid_count() {
        let urls = vec!["url1", "url2", "url3"];
        assert!(FeedsCog::validate_urls(&urls).is_ok());
    }

    #[test]
    fn test_validate_urls_rejects_too_many() {
        let urls = vec!["url"; 11];
        let result = FeedsCog::validate_urls(&urls);
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
        assert!(FeedsCog::validate_urls(&urls).is_ok());
    }
}
