//! The `/vc settings` feature shell — a [`GuiFeature`] over the pure voice
//! settings core.
//!
//! Renders the voice tracking toggle, drives the pure toggle transition, and
//! persists the settings through a real [`EffectHandler`] adapter when the
//! loop ends (back, about, or timeout) — replacing the old implicit
//! save-on-exit after `engine.run()`.

use std::sync::Arc;

use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::bot::gui::effects::EffectHandler;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::bot::navigation::Navigation;
use crate::bot::view::SelectValues;
use crate::entity::ServerSettings;
use crate::service::traits::VoiceTracker;
use crate::update::voice_settings::VoiceSettingsEffect;
use crate::update::voice_settings::VoiceSettingsModel;
use crate::update::voice_settings::VoiceSettingsMsg;
use crate::update::voice_settings::update as voice_settings_update;

/// Data-in for the voice settings feature: the guild and its loaded settings.
pub struct VoiceSettingsConfig {
    pub guild_id: u64,
    pub settings: ServerSettings,
}

action_enum! {
    SettingsVoiceAction {
        ToggleEnabled,
        #[label = "❮ Back"]
        Back,
        #[label = "🛈 About"]
        About,
    }
}

/// The voice settings feature.
pub struct VoiceSettingsFeature;

impl sealed::Sealed for VoiceSettingsFeature {}

impl GuiFeature for VoiceSettingsFeature {
    type Model = VoiceSettingsModel;
    type Msg = VoiceSettingsMsg;
    type Action = SettingsVoiceAction;
    type Effect = VoiceSettingsEffect;
    type Config = VoiceSettingsConfig;

    fn initial(config: Self::Config) -> Self::Model {
        VoiceSettingsModel::new(config.settings)
    }

    fn start_msg() -> Self::Msg {
        VoiceSettingsMsg::Start
    }

    fn timeout_msg() -> Self::Msg {
        VoiceSettingsMsg::Expired
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        voice_settings_update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        let is_enabled = model.voice_enabled();

        let status_text = format!(
            "-# **Settings > Voice**\n## Voice Tracking Settings\n\n> 🛈  {}",
            if is_enabled {
                "Voice tracking is **active**."
            } else {
                "Voice tracking is **paused**."
            }
        );

        let enabled = registry.register(SettingsVoiceAction::ToggleEnabled);
        let enabled_label = if is_enabled { "Disable" } else { "Enable" };
        let enabled_style = if is_enabled {
            ButtonStyle::Danger
        } else {
            ButtonStyle::Success
        };

        let container = component! {
            container {
                text_display { content: status_text }
                action_row {
                    button {
                        custom_id: enabled.id,
                        label: enabled_label,
                        style: enabled_style
                    }
                }
            }
        };

        let back = registry.register(SettingsVoiceAction::Back);
        let about = registry.register(SettingsVoiceAction::About);

        let nav_buttons = component! {
            action_row {
                button {
                    custom_id: back.id,
                    label: back.label,
                    style: ButtonStyle::Secondary
                }
                button {
                    custom_id: about.id,
                    label: about.label,
                    style: ButtonStyle::Secondary
                }
            }
        };

        vec![
            CreateComponent::Container(container),
            CreateComponent::ActionRow(nav_buttons),
        ]
    }

    fn translate(
        action: &Self::Action,
        _values: SelectValues,
        _model: &Self::Model,
    ) -> Option<Self::Msg> {
        match action {
            SettingsVoiceAction::ToggleEnabled => Some(VoiceSettingsMsg::ToggleEnabled),
            SettingsVoiceAction::Back => Some(VoiceSettingsMsg::Back),
            SettingsVoiceAction::About => Some(VoiceSettingsMsg::About),
        }
    }

    fn exit_navigation(msg: &Self::Msg) -> Option<Navigation> {
        match msg {
            VoiceSettingsMsg::Back => Some(Navigation::SettingsMain),
            VoiceSettingsMsg::About => Some(Navigation::SettingsAbout),
            _ => None,
        }
    }
}

/// The effect adapter that persists voice settings when the loop ends.
pub struct VoiceSettingsEffectHandler {
    service: Arc<dyn VoiceTracker>,
    guild_id: u64,
}

impl VoiceSettingsEffectHandler {
    /// Creates an adapter bound to the guild's voice tracking service.
    pub fn new(service: Arc<dyn VoiceTracker>, guild_id: u64) -> Self {
        Self { service, guild_id }
    }
}

impl EffectHandler for VoiceSettingsEffectHandler {
    type Effect = VoiceSettingsEffect;
    type Msg = VoiceSettingsMsg;

    fn execute(&mut self, effect: VoiceSettingsEffect) -> Vec<VoiceSettingsMsg> {
        match effect {
            VoiceSettingsEffect::PersistSettings(settings) => {
                let service = self.service.clone();
                let guild_id = self.guild_id;
                tokio::spawn(async move {
                    let _ = service.update_server_settings(guild_id, settings).await;
                });
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
    fn voice_settings_render_snapshot() {
        let mut settings = ServerSettings::default();
        settings.voice.enabled = Some(true);
        let model = VoiceSettingsModel::new(settings);

        let mut registry = ActionRegistry::new();
        let components = VoiceSettingsFeature::view(&model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);

        assert_eq!(
            value,
            json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "-# **Settings > Voice**\n## Voice Tracking Settings\n\n> 🛈  Voice tracking is **active**."
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:SettingsVoiceAction",
                                    "disabled": false,
                                    "label": "Disable",
                                    "style": 4
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
                            "custom_id": "id:SettingsVoiceAction",
                            "disabled": false,
                            "label": "❮ Back",
                            "style": 2
                        },
                        {
                            "type": 2,
                            "custom_id": "id:SettingsVoiceAction",
                            "disabled": false,
                            "label": "🛈 About",
                            "style": 2
                        }
                    ]
                }
            ])
        );
    }
}
