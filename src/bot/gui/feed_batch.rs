//! The feed subscription batch view shell — a [`GuiFeature`] over the pure
//! feed batch core.
//!
//! Renders the accumulated per-URL subscription results, and, in the final
//! phase, the "View Subscriptions" action that navigates to the feed list.

use pwr_ext::component;

use crate::bot::command::feed::SendInto;
use crate::bot::command::prelude::*;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::bot::navigation::Navigation;
use crate::bot::view::SelectValues;
use crate::entity::SubscriberType;
use crate::update::feed_batch::FeedBatchEffect;
use crate::update::feed_batch::FeedBatchModel;
use crate::update::feed_batch::FeedBatchMsg;
use crate::update::feed_batch::FeedBatchPhase;
use crate::update::feed_batch::update as feed_batch_update;

action_enum! {
    FeedSubscriptionBatchAction {
        #[label = "View Subscriptions"]
        ViewSubscriptions,
    }
}

/// The feed batch feature.
pub struct FeedBatchFeature;

impl sealed::Sealed for FeedBatchFeature {}

impl GuiFeature for FeedBatchFeature {
    type Model = FeedBatchModel;
    type Msg = FeedBatchMsg;
    type Action = FeedSubscriptionBatchAction;
    type Effect = FeedBatchEffect;
    type Config = FeedBatchModel;

    fn initial(config: Self::Config) -> Self::Model {
        config
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        feed_batch_update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        let text_components: Vec<CreateContainerComponent> = model
            .states
            .iter()
            .map(|s| {
                CreateContainerComponent::TextDisplay(component! {
                    text_display { content: s.clone() }
                })
            })
            .collect();

        let mut components = vec![CreateComponent::Container(CreateContainer::new(
            text_components,
        ))];

        if model.phase == FeedBatchPhase::Done {
            let nav_button = registry.register(FeedSubscriptionBatchAction::ViewSubscriptions);

            let nav_row = component! {
                action_row {
                    button {
                        custom_id: nav_button.id,
                        label: nav_button.label,
                        style: ButtonStyle::Secondary
                    }
                }
            };

            components.push(CreateComponent::ActionRow(nav_row));
        }

        components
    }

    fn translate(
        action: &Self::Action,
        _values: SelectValues,
        model: &Self::Model,
    ) -> Option<Self::Msg> {
        match action {
            FeedSubscriptionBatchAction::ViewSubscriptions => {
                Some(FeedBatchMsg::ViewSubscriptions {
                    subscriber_type: model.subscriber_type,
                })
            }
        }
    }

    fn exit_navigation(msg: &Self::Msg) -> Option<Navigation> {
        match msg {
            FeedBatchMsg::ViewSubscriptions { subscriber_type } => {
                let send_into = match subscriber_type {
                    SubscriberType::Guild => SendInto::Server,
                    SubscriberType::Dm => SendInto::DM,
                };
                Some(Navigation::FeedList(Some(send_into)))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::bot::view::ActionRegistry;
    use crate::bot::view::normalize_custom_ids;

    #[test]
    fn batch_handler_non_final_snapshot() {
        let model = FeedBatchModel::new(
            vec!["Subscribed to https://a.com".to_string()],
            FeedBatchPhase::Confirm,
            SubscriberType::Dm,
        );
        let mut registry = ActionRegistry::new();
        let components = FeedBatchFeature::view(&model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);
        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        { "type": 10, "content": "Subscribed to https://a.com" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn batch_handler_final_snapshot() {
        let model = FeedBatchModel::new(
            vec![
                "Subscribed to https://a.com".to_string(),
                "Subscribed to https://b.com".to_string(),
            ],
            FeedBatchPhase::Done,
            SubscriberType::Dm,
        );
        let mut registry = ActionRegistry::new();
        let components = FeedBatchFeature::view(&model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);
        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        { "type": 10, "content": "Subscribed to https://a.com" },
                        { "type": 10, "content": "Subscribed to https://b.com" }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 2,
                            "custom_id": "id:FeedSubscriptionBatchAction",
                            "disabled": false,
                            "label": "View Subscriptions",
                            "style": 2
                        }
                    ]
                }
            ])
        );
    }
}
