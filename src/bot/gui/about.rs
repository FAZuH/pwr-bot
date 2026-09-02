//! The `/about` feature shell — a [`GuiFeature`] over the pure about core.

use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::bot::navigation::Navigation;
use crate::bot::view::SelectValues;
use crate::update::about::AboutEffect;
use crate::update::about::AboutModel;
use crate::update::about::AboutMsg;
use crate::update::about::AboutStats;

/// Data-in for the about feature: the stats and avatar gathered by the command
/// handler before the host starts.
pub struct AboutConfig {
    pub stats: AboutStats,
    pub avatar_url: String,
}

action_enum! {
    AboutAction {
        #[label = "❮ Back"]
        Back,
    }
}

/// The about feature.
pub struct AboutFeature;

impl sealed::Sealed for AboutFeature {}

impl GuiFeature for AboutFeature {
    type Model = AboutModel;
    type Msg = AboutMsg;
    type Action = AboutAction;
    type Effect = AboutEffect;
    type Config = AboutConfig;

    fn initial(config: Self::Config) -> Self::Model {
        AboutModel::new(config.stats, config.avatar_url)
    }

    fn start_msg() -> Self::Msg {
        AboutMsg::Start
    }

    fn timeout_msg() -> Self::Msg {
        AboutMsg::Expired
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        crate::update::about::update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        let content_text = format!(
            "-# **Settings > About**\n## pwr-bot\n### Stats\n- **Uptime**: {}\n- **Servers**: {}\n- **Users**: {}\n- **Commands**: {}\n- **Latency**: {}ms\n- **Memory**: {:.1} MB\n### Info\n- **Author**: [FAZuH](https://github.com/FAZuH)\n- **Source**: [GitHub](https://github.com/FAZuH/pwr-bot)\n- **License**: [MIT](https://github.com/FAZuH/pwr-bot/blob/main/LICENSE)\nCopyright © 2025-{} FAZuH  —  v{}",
            AboutModel::format_uptime(model.stats.uptime),
            AboutModel::format_number(model.stats.guild_count),
            AboutModel::format_number(model.stats.user_count),
            model.stats.command_count,
            model.stats.latency_ms,
            model.stats.memory_mb,
            model.stats.current_year,
            model.stats.version,
        );

        let back_action = registry.register(AboutAction::Back);

        let container = component! {
            container {
                section {
                    text_display { content: content_text }
                    thumbnail { media: model.avatar_url.clone() }
                }
                action_row {
                    button { url: "https://github.com/FAZuH/pwr-bot", label: "Source Code" }
                    button { url: "https://github.com/FAZuH/pwr-bot/blob/main/LICENSE", label: "License" }
                }
            }
        };

        let back_button = component! {
            action_row {
                button {
                    custom_id: back_action.id,
                    label: back_action.label,
                    style: ButtonStyle::Secondary
                }
            }
        };

        vec![
            CreateComponent::Container(container),
            CreateComponent::ActionRow(back_button),
        ]
    }

    fn translate(
        action: &Self::Action,
        _values: SelectValues,
        _model: &Self::Model,
    ) -> Option<Self::Msg> {
        match action {
            AboutAction::Back => Some(AboutMsg::Back),
        }
    }

    fn exit_navigation(msg: &Self::Msg) -> Option<Navigation> {
        match msg {
            AboutMsg::Back => Some(Navigation::SettingsMain),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::bot::view::ActionRegistry;

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

    #[test]
    fn about_view_render_snapshot() {
        let stats = AboutStats::new(
            "0.1.0".to_string(),
            Duration::from_secs(90_000),
            2,
            150,
            42,
            12,
            320.0,
            2026,
        );
        let model = AboutModel::new(stats, "https://example.com/avatar.png".to_string());

        let mut registry = ActionRegistry::new();
        let components = AboutFeature::view(&model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);

        let expected = json!([
            {
                "type": 17,
                "components": [
                    {
                        "type": 9,
                        "components": [
                            {
                                "type": 10,
                                "content": "-# **Settings > About**\n## pwr-bot\n### Stats\n- **Uptime**: 1 days, 1 hours, 0 minutes\n- **Servers**: 2\n- **Users**: 150\n- **Commands**: 12\n- **Latency**: 42ms\n- **Memory**: 320.0 MB\n### Info\n- **Author**: [FAZuH](https://github.com/FAZuH)\n- **Source**: [GitHub](https://github.com/FAZuH/pwr-bot)\n- **License**: [MIT](https://github.com/FAZuH/pwr-bot/blob/main/LICENSE)\nCopyright © 2025-2026 FAZuH  —  v0.1.0"
                            }
                        ],
                        "accessory": {
                            "type": 11,
                            "media": { "url": "https://example.com/avatar.png" }
                        }
                    },
                    {
                        "type": 1,
                        "components": [
                            {
                                "type": 2,
                                "disabled": false,
                                "label": "Source Code",
                                "style": 5,
                                "url": "https://github.com/FAZuH/pwr-bot"
                            },
                            {
                                "type": 2,
                                "disabled": false,
                                "label": "License",
                                "style": 5,
                                "url": "https://github.com/FAZuH/pwr-bot/blob/main/LICENSE"
                            }
                        ]
                    }
                ]
            },
            {
                "type": 1,
                "components": [
                    {
                        "type": 2,
                        "custom_id": "id:AboutAction",
                        "disabled": false,
                        "label": "❮ Back",
                        "style": 2
                    }
                ]
            }
        ]);

        assert_eq!(value, expected);
    }
}
