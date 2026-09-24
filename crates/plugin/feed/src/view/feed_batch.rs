use pwr_ext::component;
use pwr_ext::view_support::ButtonStyle;
use pwr_ext::view_support::CreateComponent;
use pwr_ext::view_support::CreateContainer;
use pwr_ext::view_support::CreateContainerComponent;

use crate::update::feed_batch::FeedBatchModel;
use crate::update::feed_batch::FeedBatchPhase;

pub const VIEW_SUBSCRIPTIONS: &str = "feed-batch:view-subscriptions";

pub fn components(model: &FeedBatchModel) -> Vec<CreateComponent<'static>> {
    let text_components: Vec<CreateContainerComponent<'static>> = model
        .states
        .iter()
        .map(|state| {
            CreateContainerComponent::TextDisplay(component! {
                text_display { content: state.clone() }
            })
        })
        .collect();
    let mut components = vec![CreateComponent::Container(CreateContainer::new(
        text_components,
    ))];
    if model.phase == FeedBatchPhase::Done {
        components.push(CreateComponent::ActionRow(component! {
            action_row {
                button {
                    custom_id: VIEW_SUBSCRIPTIONS,
                    label: "View Subscriptions",
                    style: ButtonStyle::Secondary
                }
            }
        }));
    }
    components
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use serde_json::json;

    use super::*;
    use crate::entity::SubscriberType;

    fn normalize(value: &mut Value) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    if key == "custom_id" && value.as_str() == Some(VIEW_SUBSCRIPTIONS) {
                        *value = json!("id:FeedSubscriptionBatchAction");
                    } else {
                        normalize(value);
                    }
                }
            }
            Value::Array(values) => values.iter_mut().for_each(normalize),
            _ => {}
        }
    }

    fn rendered(model: &FeedBatchModel) -> Value {
        let mut value = serde_json::to_value(components(model)).unwrap();
        normalize(&mut value);
        value
    }

    #[test]
    fn processing_snapshot() {
        let model = FeedBatchModel::new(
            vec!["Subscribed to https://a.com".to_string()],
            FeedBatchPhase::Confirm,
            SubscriberType::Dm,
        );
        assert_eq!(
            rendered(&model),
            json!([{
                "type": 17,
                "components": [{ "type": 10, "content": "Subscribed to https://a.com" }]
            }])
        );
    }

    #[test]
    fn final_snapshot() {
        let model = FeedBatchModel::new(
            vec![
                "Subscribed to https://a.com".to_string(),
                "Subscribed to https://b.com".to_string(),
            ],
            FeedBatchPhase::Done,
            SubscriberType::Dm,
        );
        assert_eq!(
            rendered(&model),
            json!([{
                "type": 17,
                "components": [
                    { "type": 10, "content": "Subscribed to https://a.com" },
                    { "type": 10, "content": "Subscribed to https://b.com" }
                ]
            }, {
                "type": 1,
                "components": [{
                    "type": 2,
                    "custom_id": "id:FeedSubscriptionBatchAction",
                    "disabled": false,
                    "label": "View Subscriptions",
                    "style": 2
                }]
            }])
        );
    }
}
