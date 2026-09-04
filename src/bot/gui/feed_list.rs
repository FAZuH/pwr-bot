//! The `/feed list` feature shell — a [`GuiFeature`] over the pure feed list
//! core.
//!
//! Renders the dynamic 0..N subscription sections, the Edit/View mode toggle
//! with per-subscription unsubscribe buttons, the Save action, and the
//! pagination row. The feature holds no service or domain-data fetch: the
//! subscriptions arrive at model construction (data-in via `Config`), and
//! in-session refetches (pagination, post-save reload) are
//! [`FeedListEffect`]s executed by the [`FeedListEffectHandler`] adapter.

use std::sync::Arc;

use pwr_ext::component;

use crate::bot::command::feed::list::SUBSCRIPTIONS_PER_PAGE;
use crate::bot::command::prelude::*;
use crate::bot::gui::effects::EffectHandler;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::bot::navigation::Navigation;
use crate::bot::view::SelectValues;
use crate::bot::view::pagination::PaginationView;
use crate::entity::SubscriberEntity;
use crate::service::feed_subscription::Subscription;
use crate::service::traits::FeedSubscriptionProvider;
use crate::update::feed_list::FeedListEffect;
use crate::update::feed_list::FeedListModel;
use crate::update::feed_list::FeedListMsg;
use crate::update::feed_list::FeedListViewState;
use crate::update::feed_list::update as feed_list_update;
use crate::update::pagination::PaginationAction;

/// Data-in for the feed list feature: the first page of subscriptions.
pub struct FeedListConfig {
    pub subscriptions: Vec<Subscription>,
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
} }

/// The feed list feature.
pub struct FeedListFeature;

impl sealed::Sealed for FeedListFeature {}

impl GuiFeature for FeedListFeature {
    type Model = FeedListModel;
    type Msg = FeedListMsg;
    type Action = FeedListAction;
    type Effect = FeedListEffect;
    type Config = FeedListConfig;

    fn initial(config: Self::Config) -> Self::Model {
        FeedListModel::new(config.subscriptions, SUBSCRIPTIONS_PER_PAGE)
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        feed_list_update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        if model.subscriptions().is_empty() {
            create_empty()
        } else {
            let sections: Vec<CreateContainerComponent<'a>> = model
                .subscriptions()
                .iter()
                .map(|sub| create_subscription_section(model, registry, sub))
                .collect();

            let container = CreateComponent::Container(CreateContainer::new(sections));
            let mut components = vec![container];

            let mut pagination =
                PaginationView::new(model.subscriptions().len() as u32, model.per_page());
            pagination.state.current_page = model.current_page();
            pagination.disabled = model.pagination_disabled();
            pagination.attach_if_multipage(registry, &mut components, FeedListAction::Base);
            components.push(create_toggle_button(model, registry));

            components
        }
    }

    fn translate(
        action: &Self::Action,
        _values: SelectValues,
        _model: &Self::Model,
    ) -> Option<Self::Msg> {
        match action {
            FeedListAction::Base(inner) => Some(FeedListMsg::Pagination(*inner)),
            FeedListAction::Edit => Some(FeedListMsg::Edit),
            FeedListAction::View => Some(FeedListMsg::View),
            FeedListAction::Unsubscribe { source_url } => Some(FeedListMsg::ToggleUnsub {
                source_url: source_url.clone(),
            }),
            FeedListAction::UndoUnsub { source_url } => Some(FeedListMsg::ToggleUnsub {
                source_url: source_url.clone(),
            }),
            FeedListAction::Save => Some(FeedListMsg::Save),
        }
    }

    fn exit_navigation(_msg: &Self::Msg) -> Option<Navigation> {
        None
    }
}

/// Creates an empty-state view.
fn create_empty<'a>() -> Vec<CreateComponent<'a>> {
    vec![CreateComponent::Container(component! {
        container {
            text_display { content: "You have no subscriptions." }
        }
    })]
}

/// Creates a section component for a single subscription.
fn create_subscription_section<'a>(
    model: &'a FeedListModel,
    registry: &mut ActionRegistry<FeedListAction>,
    sub: &Subscription,
) -> CreateContainerComponent<'a> {
    let text = if let Some(latest) = sub.feed_latest.as_ref() {
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

    match model.state() {
        FeedListViewState::View => CreateContainerComponent::Section(component! {
            section {
                text_display { content: text }
                thumbnail { media: sub.feed.cover_url.clone() }
            }
        }),
        FeedListViewState::Edit => {
            let source_url = sub.feed.source_url.clone();
            let marked = model.marked_unsub().contains(&source_url);
            let action = if marked {
                registry.register(FeedListAction::UndoUnsub { source_url })
            } else {
                registry.register(FeedListAction::Unsubscribe { source_url })
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

/// Creates the bottom toggle/save action row.
fn create_toggle_button<'a>(
    model: &FeedListModel,
    registry: &mut ActionRegistry<FeedListAction>,
) -> CreateComponent<'a> {
    let action = match model.state() {
        FeedListViewState::Edit => FeedListAction::View,
        FeedListViewState::View => FeedListAction::Edit,
    };

    let state_button = registry.register(action);
    let save_button = registry.register(FeedListAction::Save);
    let save_disabled = model.marked_unsub().is_empty();

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

/// The effect adapter that executes feed-list refetches and unsubscribes.
pub struct FeedListEffectHandler {
    service: Arc<dyn FeedSubscriptionProvider>,
    subscriber: SubscriberEntity,
}

impl FeedListEffectHandler {
    /// Creates an adapter bound to the subscriber's feed subscription service.
    pub fn new(service: Arc<dyn FeedSubscriptionProvider>, subscriber: SubscriberEntity) -> Self {
        Self {
            service,
            subscriber,
        }
    }
}

impl EffectHandler for FeedListEffectHandler {
    type Effect = FeedListEffect;
    type Msg = FeedListMsg;

    fn execute(
        &mut self,
        effect: FeedListEffect,
        tx: tokio::sync::mpsc::UnboundedSender<FeedListMsg>,
    ) -> Vec<FeedListMsg> {
        match effect {
            FeedListEffect::QuerySubscriptions { page, per_page } => {
                let service = self.service.clone();
                let subscriber = self.subscriber.clone();
                tokio::spawn(async move {
                    let subs = service
                        .list_paginated_subscriptions(&subscriber, page, per_page)
                        .await
                        .unwrap_or_else(|e| {
                            log::error!("feed list refetch failed: {e}");
                            Vec::new()
                        });
                    let _ = tx.send(FeedListMsg::SubscriptionsLoaded(subs));
                });
                vec![]
            }
            FeedListEffect::SaveUnsubscribes(urls) => {
                let service = self.service.clone();
                let subscriber = self.subscriber.clone();
                tokio::spawn(async move {
                    for url in urls {
                        if let Err(e) = service.unsubscribe(&url, &subscriber).await {
                            log::error!("feed unsubscribe failed for {url}: {e}");
                        }
                    }
                    let _ = tx.send(FeedListMsg::Saved);
                });
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use serde_json::json;

    use super::*;
    use crate::bot::view::ActionRegistry;
    use crate::entity::FeedEntity;
    use crate::entity::FeedItemEntity;

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
                        let replacement = json!(format!("id:{}", parts[0]));
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
                published: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            }),
        }
    }

    #[test]
    fn feed_list_empty_snapshot() {
        let model = FeedListModel::new(vec![], SUBSCRIPTIONS_PER_PAGE);
        let mut registry = ActionRegistry::new();
        let components = FeedListFeature::view(&model, &mut registry);
        let value = serde_json::to_value(&components).unwrap();
        assert_eq!(
            value,
            json!([
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
        let model = FeedListModel::new(
            vec![
                make_sub("Alpha", "https://alpha.example.com/feed"),
                make_sub("Beta", "https://beta.example.com/feed"),
            ],
            SUBSCRIPTIONS_PER_PAGE,
        );
        let mut registry = ActionRegistry::new();
        let components = FeedListFeature::view(&model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);
        assert_eq!(
            value,
            json!([
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
        let mut model = FeedListModel::new(
            vec![make_sub("Alpha", "https://alpha.example.com/feed")],
            SUBSCRIPTIONS_PER_PAGE,
        );
        FeedListFeature::update(FeedListMsg::Edit, &mut model);
        let mut registry = ActionRegistry::new();
        let components = FeedListFeature::view(&model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);
        assert_eq!(
            value,
            json!([
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
