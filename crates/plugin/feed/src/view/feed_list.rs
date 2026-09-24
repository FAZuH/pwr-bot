use pwr_ext::component;
use pwr_ext::view_support::ButtonStyle;
use pwr_ext::view_support::CreateComponent;
use pwr_ext::view_support::CreateContainer;
use pwr_ext::view_support::CreateContainerComponent;

use crate::service::feed_subscription::Subscription;
use crate::update::feed_list::FeedListModel;
use crate::update::feed_list::FeedListViewState;

pub const EDIT: &str = "feed-list:edit";
pub const VIEW: &str = "feed-list:view";
pub const SAVE: &str = "feed-list:save";
pub const UNSUBSCRIBE_PREFIX: &str = "feed-list:unsub:";
pub const FIRST: &str = "feed-list:first";
pub const PREVIOUS: &str = "feed-list:previous";
pub const NEXT: &str = "feed-list:next";
pub const LAST: &str = "feed-list:last";

pub fn components(model: &FeedListModel) -> Vec<CreateComponent<'static>> {
    if model.subscriptions().is_empty() {
        return vec![CreateComponent::Container(component! {
            container {
                text_display { content: "You have no subscriptions." }
            }
        })];
    }

    let sections: Vec<CreateContainerComponent<'static>> = model
        .subscriptions()
        .iter()
        .enumerate()
        .map(|(index, subscription)| subscription_section(model, index, subscription))
        .collect();
    let mut components = vec![CreateComponent::Container(CreateContainer::new(sections))];
    if let Some(pagination) = pagination_component(model) {
        components.push(pagination);
    }
    components.push(toggle_component(model));
    components
}

fn subscription_section(
    model: &FeedListModel,
    index: usize,
    subscription: &Subscription,
) -> CreateContainerComponent<'static> {
    let text = if let Some(latest) = subscription.feed_latest.as_ref() {
        format!(
            "### {}\n\n- **Last version**: {}\n- **Last updated**: <t:{}>\n- [**Source** 🗗](<{}>)",
            subscription.feed.name,
            latest.description,
            latest.published.timestamp(),
            subscription.feed.source_url
        )
    } else {
        format!(
            "### {}\n\n> No latest version found.\n- [**Source** 🗗](<{}>)",
            subscription.feed.name, subscription.feed.source_url
        )
    };
    match model.state() {
        FeedListViewState::View => CreateContainerComponent::Section(component! {
            section {
                text_display { content: text }
                thumbnail { media: subscription.feed.cover_url.clone() }
            }
        }),
        FeedListViewState::Edit => {
            let source_url = subscription.feed.source_url.as_str();
            let (custom_id, label, style) = if model.marked_unsub().contains(source_url) {
                (
                    format!("{UNSUBSCRIBE_PREFIX}{index}:undo"),
                    "↶ Undo",
                    ButtonStyle::Secondary,
                )
            } else {
                (
                    format!("{UNSUBSCRIBE_PREFIX}{index}"),
                    "🗑 Unsubscribe",
                    ButtonStyle::Danger,
                )
            };
            CreateContainerComponent::Section(component! {
                section {
                    text_display { content: text }
                    button {
                        custom_id: custom_id,
                        label: label,
                        style: style
                    }
                }
            })
        }
    }
}

fn toggle_component(model: &FeedListModel) -> CreateComponent<'static> {
    let (custom_id, label) = match model.state() {
        FeedListViewState::Edit => (VIEW, "👁 View Mode"),
        FeedListViewState::View => (EDIT, "✎ Edit Subscriptions"),
    };
    CreateComponent::ActionRow(component! {
        action_row {
            button {
                custom_id: custom_id,
                label: label,
                style: ButtonStyle::Primary
            }
            button {
                custom_id: SAVE,
                label: "Save",
                style: ButtonStyle::Success,
                disabled: model.marked_unsub().is_empty()
            }
        }
    })
}

fn pagination_component(model: &FeedListModel) -> Option<CreateComponent<'static>> {
    let pages = (model.subscriptions().len() as u32).div_ceil(model.per_page());
    if model.pagination_disabled() || pages <= 1 {
        return None;
    }
    let current_page = model.current_page();
    Some(CreateComponent::ActionRow(component! {
        action_row {
            button {
                custom_id: FIRST,
                label: "⏮",
                style: ButtonStyle::Primary,
                disabled: current_page == 1
            }
            button {
                custom_id: PREVIOUS,
                label: "◀",
                style: ButtonStyle::Primary,
                disabled: current_page == 1
            }
            button {
                custom_id: "current",
                label: format!("{current_page}/{pages}"),
                style: ButtonStyle::Secondary,
                disabled: true
            }
            button {
                custom_id: NEXT,
                label: "▶",
                style: ButtonStyle::Primary,
                disabled: current_page == pages
            }
            button {
                custom_id: LAST,
                label: "⏭",
                style: ButtonStyle::Primary,
                disabled: current_page == pages
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use serde_json::Value;
    use serde_json::json;

    use super::*;
    use crate::entity::FeedEntity;
    use crate::entity::FeedItemEntity;
    use crate::update::feed_list::FeedListMsg;
    use crate::update::feed_list::update;

    fn subscription(name: &str, url: &str) -> Subscription {
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

    fn normalize(value: &mut Value) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    if key == "custom_id"
                        && let Some(custom_id) = value.as_str()
                        && custom_id.starts_with("feed-list:")
                        && custom_id != "current"
                    {
                        *value = json!("id:FeedListAction");
                    } else {
                        normalize(value);
                    }
                }
            }
            Value::Array(values) => values.iter_mut().for_each(normalize),
            _ => {}
        }
    }

    fn rendered(model: &FeedListModel) -> Value {
        let mut value = serde_json::to_value(components(model)).unwrap();
        normalize(&mut value);
        value
    }

    #[test]
    fn empty_snapshot() {
        let model = FeedListModel::new(vec![], 10);
        assert_eq!(
            rendered(&model),
            json!([{
                "type": 17,
                "components": [{ "type": 10, "content": "You have no subscriptions." }]
            }])
        );
    }

    #[test]
    fn view_mode_snapshot() {
        let model = FeedListModel::new(
            vec![
                subscription("Alpha", "https://alpha.example.com/feed"),
                subscription("Beta", "https://beta.example.com/feed"),
            ],
            10,
        );
        assert_eq!(
            rendered(&model),
            json!([{
                "type": 17,
                "components": [
                    {
                        "type": 9,
                        "components": [{
                            "type": 10,
                            "content": "### Alpha\n\n- **Last version**: v1\n- **Last updated**: <t:1700000000>\n- [**Source** 🗗](<https://alpha.example.com/feed>)"
                        }],
                        "accessory": {
                            "type": 11,
                            "media": { "url": "https://cover.example.com/Alpha.png" }
                        }
                    },
                    {
                        "type": 9,
                        "components": [{
                            "type": 10,
                            "content": "### Beta\n\n- **Last version**: v1\n- **Last updated**: <t:1700000000>\n- [**Source** 🗗](<https://beta.example.com/feed>)"
                        }],
                        "accessory": {
                            "type": 11,
                            "media": { "url": "https://cover.example.com/Beta.png" }
                        }
                    }
                ]
            }, {
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
            }])
        );
    }

    #[test]
    fn edit_mode_snapshot() {
        let mut model = FeedListModel::new(
            vec![subscription("Alpha", "https://alpha.example.com/feed")],
            10,
        );
        update(FeedListMsg::Edit, &mut model);
        assert_eq!(
            rendered(&model),
            json!([{
                "type": 17,
                "components": [{
                    "type": 9,
                    "components": [{
                        "type": 10,
                        "content": "### Alpha\n\n- **Last version**: v1\n- **Last updated**: <t:1700000000>\n- [**Source** 🗗](<https://alpha.example.com/feed>)"
                    }],
                    "accessory": {
                        "type": 2,
                        "custom_id": "id:FeedListAction",
                        "disabled": false,
                        "label": "🗑 Unsubscribe",
                        "style": 4
                    }
                }]
            }, {
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
            }])
        );
    }

    #[test]
    fn edit_action_switches_the_rendered_view_to_edit_mode() {
        let mut model = FeedListModel::new(
            vec![subscription("Alpha", "https://alpha.example.com/feed")],
            10,
        );
        let before = serde_json::to_value(components(&model)).unwrap();
        let edit_id = before[1]["components"][0]["custom_id"]
            .as_str()
            .expect("edit action custom id");
        assert_eq!(edit_id, EDIT);
        assert_eq!(
            before[1]["components"][0]["label"],
            json!("✎ Edit Subscriptions")
        );

        let message = crate::command::translate_list_action(edit_id, &model).unwrap();
        assert!(matches!(message, FeedListMsg::Edit));
        let effects = update(message, &mut model);
        assert!(effects.is_empty());
        assert_eq!(model.state(), FeedListViewState::Edit);

        let after = serde_json::to_value(components(&model)).unwrap();
        assert_eq!(after[1]["components"][0]["label"], json!("👁 View Mode"));
        assert_eq!(after[1]["components"][0]["custom_id"], json!(VIEW));
    }

    #[test]
    fn multiple_pages_render_the_pagination_row() {
        let subscriptions = (0..11)
            .map(|index| {
                subscription(
                    &format!("Feed {index}"),
                    &format!("https://example.com/{index}"),
                )
            })
            .collect();
        let model = FeedListModel::new(subscriptions, 10);
        let value = rendered(&model);

        assert_eq!(value[1]["components"].as_array().unwrap().len(), 5);
        assert_eq!(value[1]["components"][2]["label"], json!("1/2"));
    }
}
