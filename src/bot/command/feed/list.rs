//! Feed list subcommand.
use std::time::Duration;

use pwr_ext::component;

use crate::bot::command::feed::SendInto;
use crate::bot::command::feed::get_or_create_subscriber;
use crate::bot::command::prelude::*;
use crate::entity::SubscriberEntity;
use crate::service::feed_subscription::Subscription;
use crate::service::traits::FeedSubscriptionProvider;
use crate::update::Update;
use crate::update::feed_list::FeedListCmd;
use crate::update::feed_list::FeedListModel;
use crate::update::feed_list::FeedListMsg;
use crate::update::feed_list::FeedListUpdate;
use crate::update::feed_list::FeedListViewState;

/// Number of items per page for subscriptions list.
pub(crate) const SUBSCRIPTIONS_PER_PAGE: u32 = 10;

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
    let sent_into = sent_into.unwrap_or(SendInto::DM);
    Router::new(ctx)
        .run(Navigation::FeedList(Some(sent_into)))
        .await?;
    Ok(())
}

handler! { pub struct FeedListHandler<'a> {
    send_into: SendInto
} }

#[async_trait::async_trait]
impl CommandHandler for FeedListHandler<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let subscriber = get_or_create_subscriber(ctx, &self.send_into).await?;

        let service = ctx.data().service.feed_subscription.clone();

        let subscriptions = service
            .list_paginated_subscriptions(&subscriber, 1u32, SUBSCRIPTIONS_PER_PAGE)
            .await?;

        let view = FeedListView {
            subscriptions,
            model: FeedListModel::new(SUBSCRIPTIONS_PER_PAGE),
            service: service.clone(),
            subscriber: subscriber.clone(),
        };

        let mut engine = ViewEngine::new(ctx, view, Duration::from_secs(120), coordinator.clone());

        engine.run().await?;

        Ok(())
    }
}

pub struct FeedListView {
    pub subscriptions: Vec<Subscription>,
    pub model: FeedListModel,
    pub service: std::sync::Arc<dyn FeedSubscriptionProvider>,
    pub subscriber: SubscriberEntity,
}

impl FeedListView {
    /// Creates an empty state view.
    fn create_empty<'a>() -> Vec<CreateComponent<'a>> {
        vec![CreateComponent::Container(component! {
            container {
                text_display { content: "You have no subscriptions." }
            }
        })]
    }

    /// Creates a section component for a single subscription.
    fn create_subscription_section<'a>(
        &self,
        registry: &mut ActionRegistry<FeedListAction>,
        sub: Subscription,
    ) -> CreateContainerComponent<'a> {
        use FeedListAction::*;
        let text = if let Some(latest) = sub.feed_latest {
            format!(
                "### {}\n\n- **Last version**: {}\n- **Last updated**: <t:{}>\n- [**Source** 🗗](<{}>)",
                sub.feed.name,
                latest.description,
                latest.published.timestamp(),
                sub.feed.source_url
            )
        } else {
            format!(
                "### {}\n\n> No latest version found.\n- [**Source** 🗗](<{}>)",
                sub.feed.name, sub.feed.source_url
            )
        };

        match self.model.state {
            FeedListViewState::View => CreateContainerComponent::Section(component! {
                section {
                    text_display { content: text }
                    thumbnail { media: sub.feed.cover_url }
                }
            }),
            FeedListViewState::Edit => {
                let source_url = sub.feed.source_url;
                let marked = self.model.marked_unsub.contains(&source_url);
                let action = if marked {
                    registry.register(UndoUnsub { source_url })
                } else {
                    registry.register(Unsubscribe { source_url })
                };
                let style = if marked {
                    ButtonStyle::Secondary
                } else {
                    ButtonStyle::Danger
                };
                CreateContainerComponent::Section(component! {
                    section {
                        text_display { content: text }
                        button {
                            custom_id: action.id,
                            label: action.label,
                            style: style
                        }
                    }
                })
            }
        }
    }

    /// Create button section of the view at the bottom.
    fn create_toggle_button<'a>(
        &self,
        registry: &mut ActionRegistry<FeedListAction>,
    ) -> CreateComponent<'a> {
        let action = match self.model.state {
            FeedListViewState::Edit => FeedListAction::View,
            FeedListViewState::View => FeedListAction::Edit,
        };

        let state_button = registry.register(action);
        let save_button = registry.register(FeedListAction::Save);
        let save_disabled = self.model.marked_unsub.is_empty();

        CreateComponent::ActionRow(component! {
            action_row {
                button {
                    custom_id: state_button.id,
                    label: state_button.label,
                    style: ButtonStyle::Primary
                }
                button {
                    custom_id: save_button.id,
                    label: save_button.label,
                    style: ButtonStyle::Success,
                    disabled: save_disabled
                }
            }
        })
    }

    async fn update_subs(&mut self) -> Result<(), Error> {
        let subs = self
            .service
            .list_paginated_subscriptions(
                &self.subscriber,
                self.model.current_page,
                self.model.per_page,
            )
            .await?;
        self.subscriptions = subs;
        Ok(())
    }
}

impl ViewRender for FeedListView {
    type Action = FeedListAction;
    fn render(&self, registry: &mut ActionRegistry<FeedListAction>) -> ResponseKind<'_> {
        let components = if self.subscriptions.is_empty() {
            FeedListView::create_empty()
        } else {
            let sections: Vec<CreateContainerComponent<'_>> = self
                .subscriptions
                .clone()
                .into_iter()
                .map(|sub| self.create_subscription_section(registry, sub))
                .collect();

            let container = CreateComponent::Container(CreateContainer::new(sections));
            let mut components = vec![container];

            let mut pagination =
                PaginationView::new(self.subscriptions.len() as u32, self.model.per_page);
            pagination.state.current_page = self.model.current_page;
            pagination.disabled = self.model.pagination_disabled;
            pagination.attach_if_multipage(registry, &mut components, FeedListAction::Base);
            components.push(self.create_toggle_button(registry));

            components
        };

        components.into()
    }
}

action_extends! { FeedListAction extends PaginationAction {
    #[label = "✎ Edit Subscriptions"]
    Edit,
    #[label = "👁 View Mode"]
    View,
    #[label = "🗑 Unsubscribe"]
    Unsubscribe { source_url: String },
    #[label = "↶ Undo"]
    UndoUnsub { source_url: String },
    Save,
    Exit,
}}

#[async_trait::async_trait]
impl ViewHandler for FeedListView {
    type Action = FeedListAction;
    async fn handle(&mut self, ctx: ViewContext<'_, FeedListAction>) -> Result<ViewCmd, Error> {
        use FeedListAction::*;
        match ctx.action() {
            Base(inner) => {
                FeedListUpdate::update(FeedListMsg::Pagination(*inner), &mut self.model);
                self.update_subs().await?;
                return Ok(ViewCmd::Render);
            }
            Edit => {
                FeedListUpdate::update(FeedListMsg::Edit, &mut self.model);
            }
            View => {
                FeedListUpdate::update(FeedListMsg::View, &mut self.model);
            }
            Unsubscribe { source_url } => {
                FeedListUpdate::update(
                    FeedListMsg::ToggleUnsub {
                        source_url: source_url.clone(),
                    },
                    &mut self.model,
                );
            }
            UndoUnsub { source_url } => {
                FeedListUpdate::update(
                    FeedListMsg::ToggleUnsub {
                        source_url: source_url.clone(),
                    },
                    &mut self.model,
                );
            }
            Exit => return Ok(ViewCmd::Continue),
            Save => {
                let cmd = FeedListUpdate::update(FeedListMsg::Save, &mut self.model);
                match cmd {
                    FeedListCmd::SaveUnsubscribes(urls) => {
                        for sub in urls {
                            self.service.unsubscribe(&sub, &self.subscriber).await?;
                        }
                        self.update_subs().await?;
                    }
                    FeedListCmd::RefetchSubscriptions => {
                        self.update_subs().await?;
                    }
                    FeedListCmd::None => {}
                }
            }
        };

        Ok(ViewCmd::Render)
    }

    async fn on_timeout(&mut self) -> Result<ViewCmd, Error> {
        self.model.pagination_disabled = true;
        if self.subscriptions.is_empty() {
            Ok(ViewCmd::Exit)
        } else {
            Ok(ViewCmd::RenderOnce)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::bot::view::ActionRegistry;
    use crate::bot::view::ResponseKind;
    use crate::entity::FeedEntity;
    use crate::entity::FeedItemEntity;
    use crate::entity::ServerSettings;
    use crate::entity::SubscriberType;
    use crate::service::error::ServiceError;
    use crate::service::feed_subscription::FeedUpdateResult;
    use crate::service::feed_subscription::SubscribeResult;
    use crate::service::feed_subscription::SubscriberTarget;
    use crate::service::feed_subscription::UnsubscribeResult;
    use crate::service::traits::FeedSubscriptionProvider;

    /// Rewrites every `custom_id` of the shape `Type:timestamp:counter` to a
    /// stable sentinel `id:Type`, so the rendered shape is reproducible across
    /// runs while still pinning kind/label/style/prefix/order.
    fn normalize_custom_ids(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::String(cid)) = map.get("custom_id") {
                    let parts: Vec<&str> = cid.split(':').collect();
                    if parts.len() == 3
                        && parts[1].chars().all(|c| c.is_ascii_digit())
                        && parts[2].chars().all(|c| c.is_ascii_digit())
                    {
                        let replacement = serde_json::json!(format!("id:{}", parts[0]));
                        map.insert("custom_id".to_string(), replacement);
                    }
                }
                for v in map.values_mut() {
                    normalize_custom_ids(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    normalize_custom_ids(v);
                }
            }
            _ => {}
        }
    }

    /// A stub provider that is never invoked during rendering — it only
    /// satisfies the `service` field of `FeedListView`.
    struct StubFeedProvider;

    #[async_trait::async_trait]
    impl FeedSubscriptionProvider for StubFeedProvider {
        async fn subscribe(
            &self,
            _: &str,
            _: &SubscriberEntity,
        ) -> Result<SubscribeResult, ServiceError> {
            unimplemented!()
        }
        async fn get_feeds_by_tag(&self, _: &str) -> Result<Vec<FeedEntity>, ServiceError> {
            unimplemented!()
        }
        async fn get_both_subscribers(
            &self,
            _: String,
            _: Option<String>,
        ) -> (Option<SubscriberEntity>, Option<SubscriberEntity>) {
            unimplemented!()
        }
        async fn search_and_combine_feeds(
            &self,
            _: &str,
            _: Option<SubscriberEntity>,
            _: Option<SubscriberEntity>,
        ) -> Vec<FeedEntity> {
            unimplemented!()
        }
        async fn check_feed_update(
            &self,
            _: &FeedEntity,
        ) -> Result<FeedUpdateResult, ServiceError> {
            unimplemented!()
        }
        async fn unsubscribe(
            &self,
            _: &str,
            _: &SubscriberEntity,
        ) -> Result<UnsubscribeResult, ServiceError> {
            unimplemented!()
        }
        async fn list_paginated_subscriptions(
            &self,
            _: &SubscriberEntity,
            _: u32,
            _: u32,
        ) -> Result<Vec<Subscription>, ServiceError> {
            unimplemented!()
        }
        async fn get_subscription_count(&self, _: &SubscriberEntity) -> Result<u32, ServiceError> {
            unimplemented!()
        }
        async fn search_subcriptions(
            &self,
            _: &SubscriberEntity,
            _: &str,
        ) -> Result<Vec<FeedEntity>, ServiceError> {
            unimplemented!()
        }
        async fn get_or_create_feed(&self, _: &str) -> Result<FeedEntity, ServiceError> {
            unimplemented!()
        }
        async fn get_or_create_subscriber(
            &self,
            _: &SubscriberTarget,
        ) -> Result<SubscriberEntity, ServiceError> {
            unimplemented!()
        }
        async fn get_feed_by_source_url(
            &self,
            _: &str,
        ) -> Result<Option<FeedEntity>, ServiceError> {
            unimplemented!()
        }
        async fn get_server_settings(&self, _: u64) -> Result<ServerSettings, ServiceError> {
            unimplemented!()
        }
        async fn get_subscribers_by_type_and_feed(
            &self,
            _: SubscriberType,
            _: i32,
        ) -> Result<Vec<SubscriberEntity>, ServiceError> {
            unimplemented!()
        }
        async fn update_server_settings(
            &self,
            _: u64,
            _: ServerSettings,
        ) -> Result<(), ServiceError> {
            unimplemented!()
        }
    }

    fn make_sub(name: &str, url: &str) -> Subscription {
        Subscription {
            feed: FeedEntity {
                id: 1,
                name: name.to_string(),
                description: "desc".to_string(),
                platform_id: "anilist".to_string(),
                source_id: "src".to_string(),
                items_id: "items".to_string(),
                source_url: url.to_string(),
                cover_url: format!("https://cover.example.com/{name}.png"),
                tags: String::new(),
            },
            feed_latest: Some(FeedItemEntity {
                id: 1,
                feed_id: 1,
                description: "v1".to_string(),
                published: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            }),
        }
    }

    fn view(subscriptions: Vec<Subscription>, model: FeedListModel) -> FeedListView {
        FeedListView {
            subscriptions,
            model,
            service: Arc::new(StubFeedProvider),
            subscriber: SubscriberEntity::default(),
        }
    }

    #[test]
    fn feed_list_empty_snapshot() {
        let model = FeedListModel::new(SUBSCRIPTIONS_PER_PAGE);
        let view = view(vec![], model);
        let mut registry = ActionRegistry::new();
        let response = view.render(&mut registry);
        let ResponseKind::Component(components) = response else {
            panic!("expected a component response");
        };
        let value = serde_json::to_value(&components).unwrap();
        assert_eq!(
            value,
            serde_json::json!([
                {
                    "type": 17,
                    "components": [
                        { "type": 10, "content": "You have no subscriptions." }
                    ]
                }
            ])
        );
    }

    #[test]
    fn feed_list_view_mode_snapshot() {
        let model = FeedListModel::new(SUBSCRIPTIONS_PER_PAGE);
        let subscriptions = vec![
            make_sub("Alpha", "https://alpha.example.com/feed"),
            make_sub("Beta", "https://beta.example.com/feed"),
        ];
        let view = view(subscriptions, model);
        let mut registry = ActionRegistry::new();
        let response = view.render(&mut registry);
        let ResponseKind::Component(components) = response else {
            panic!("expected a component response");
        };
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);
        assert_eq!(
            value,
            serde_json::json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 9,
                            "components": [
                                {
                                    "type": 10,
                                    "content": "### Alpha\n\n- **Last version**: v1\n- **Last updated**: <t:1700000000>\n- [**Source** 🗗](<https://alpha.example.com/feed>)"
                                }
                            ],
                            "accessory": {
                                "type": 11,
                                "media": { "url": "https://cover.example.com/Alpha.png" }
                            }
                        },
                        {
                            "type": 9,
                            "components": [
                                {
                                    "type": 10,
                                    "content": "### Beta\n\n- **Last version**: v1\n- **Last updated**: <t:1700000000>\n- [**Source** 🗗](<https://beta.example.com/feed>)"
                                }
                            ],
                            "accessory": {
                                "type": 11,
                                "media": { "url": "https://cover.example.com/Beta.png" }
                            }
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 2,
                            "custom_id": "id:FeedListAction",
                            "disabled": false,
                            "label": "✎ Edit Subscriptions",
                            "style": 1
                        },
                        {
                            "type": 2,
                            "custom_id": "id:FeedListAction",
                            "disabled": true,
                            "label": "Save",
                            "style": 3
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn feed_list_edit_mode_snapshot() {
        let mut model = FeedListModel::new(SUBSCRIPTIONS_PER_PAGE);
        model.state = FeedListViewState::Edit;
        let subscriptions = vec![make_sub("Alpha", "https://alpha.example.com/feed")];
        let view = view(subscriptions, model);
        let mut registry = ActionRegistry::new();
        let response = view.render(&mut registry);
        let ResponseKind::Component(components) = response else {
            panic!("expected a component response");
        };
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);
        assert_eq!(
            value,
            serde_json::json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 9,
                            "components": [
                                {
                                    "type": 10,
                                    "content": "### Alpha\n\n- **Last version**: v1\n- **Last updated**: <t:1700000000>\n- [**Source** 🗗](<https://alpha.example.com/feed>)"
                                }
                            ],
                            "accessory": {
                                "type": 2,
                                "custom_id": "id:FeedListAction",
                                "disabled": false,
                                "label": "🗑 Unsubscribe",
                                "style": 4
                            }
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 2,
                            "custom_id": "id:FeedListAction",
                            "disabled": false,
                            "label": "👁 View Mode",
                            "style": 1
                        },
                        {
                            "type": 2,
                            "custom_id": "id:FeedListAction",
                            "disabled": true,
                            "label": "Save",
                            "style": 3
                        }
                    ]
                }
            ])
        );
    }
}
