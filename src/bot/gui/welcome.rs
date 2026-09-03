//! The `/welcome` feature shell — a [`GuiFeature`] over the pure welcome
//! settings core.
//!
//! Renders the welcome-card toggle, the welcome-channel select, the template
//! select, the color/message buttons, the conditional message-removal select
//! with its save/cancel row, and the nav row. The initial settings and preview
//! image arrive at model construction (data-in via `Config`); in-session
//! persistence and preview regeneration are [`WelcomeSettingsEffect`]s
//! executed by the [`WelcomeSettingsEffectHandler`] adapter, whose results
//! flow back as `SettingsPersisted` / `ImageRendered` messages.

use std::borrow::Cow;
use std::str::FromStr;
use std::sync::Arc;

use poise::serenity_prelude::small_fixed_array::FixedString;
use pwr_ext::component;

use crate::bot::command::prelude::*;
use crate::bot::command::welcome::image_generator::WelcomeCardData;
use crate::bot::command::welcome::image_generator::WelcomeImageGenerator;
use crate::bot::gui::effects::EffectHandler;
use crate::bot::gui::feature::GuiFeature;
use crate::bot::gui::feature::sealed;
use crate::bot::navigation::Navigation;
use crate::bot::view::SelectValues;
use crate::entity::ServerSettings;
use crate::service::traits::FeedSubscriptionProvider;
use crate::update::welcome_settings::WelcomeSettingsEffect;
use crate::update::welcome_settings::WelcomeSettingsModel;
use crate::update::welcome_settings::WelcomeSettingsMsg;
use crate::update::welcome_settings::update as welcome_settings_update;

/// Filename for the welcome preview image attachment.
pub const WELCOME_FILE: &str = "welcome_preview.png";

/// Data-in for the welcome feature: the loaded settings and the preview image
/// generated once at boot.
pub struct WelcomeConfig {
    pub settings: ServerSettings,
    pub image_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Modal, Clone, PartialEq, Eq)]
#[name = "Add Welcome Message"]
pub struct AddWelcomeMessageModal {
    #[name = "Message"]
    #[placeholder = "Welcome to {{ server_name }}, {{ user_tag }}!"]
    #[paragraph]
    #[min_length = 1]
    #[max_length = 200]
    message: FixedString<u16>,
}

#[derive(Debug, Modal, Clone, PartialEq, Eq)]
#[name = "Set Primary Color"]
pub struct SetPrimaryColorModal {
    #[name = "Primary Color (Hex)"]
    #[placeholder = "#5865F2"]
    #[min_length = 4]
    #[max_length = 7]
    color: FixedString<u16>,
}

action_enum! {
    SettingsWelcomeAction {
        ToggleEnabled,
        ChannelSelect,
        TemplateSelect,
        #[label = "Set Color"]
        SetColor(Option<SetPrimaryColorModal>),
        MarkRemoval,
        #[label = "Add Welcome Message"]
        AddMessage(Option<AddWelcomeMessageModal>),
        #[label = "Save Removals"]
        SaveRemoval,
        #[label = "Cancel"]
        CancelRemoval,
        #[label = "❮ Back"]
        Back,
        #[label = "🛈 About"]
        About,
    }
}

/// The welcome settings feature.
pub struct WelcomeFeature;

impl sealed::Sealed for WelcomeFeature {}

impl GuiFeature for WelcomeFeature {
    type Model = WelcomeSettingsModel;
    type Msg = WelcomeSettingsMsg;
    type Action = SettingsWelcomeAction;
    type Effect = WelcomeSettingsEffect;
    type Config = WelcomeConfig;

    fn initial(config: Self::Config) -> Self::Model {
        WelcomeSettingsModel::new(config.settings, config.image_bytes)
    }

    fn start_msg() -> Self::Msg {
        WelcomeSettingsMsg::Start
    }

    fn timeout_msg() -> Self::Msg {
        WelcomeSettingsMsg::Expired
    }

    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect> {
        welcome_settings_update(msg, model)
    }

    fn view<'a>(
        model: &'a Self::Model,
        registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>> {
        let is_enabled = model.is_enabled();
        let msgs = model.message_count();

        let status_text = format!(
            "-# **Settings > Welcome**\n## Welcome Settings\n\n> 🛈  {}",
            if is_enabled {
                "Welcome cards are **active**."
            } else {
                "Welcome cards are **disabled**."
            }
        );

        let enabled_label = if is_enabled { "Disable" } else { "Enable" };
        let enabled_style = if is_enabled {
            ButtonStyle::Danger
        } else {
            ButtonStyle::Success
        };
        let toggle_action = registry.register(SettingsWelcomeAction::ToggleEnabled);

        let default_channels = model
            .settings
            .welcome
            .channel_id
            .as_deref()
            .and_then(|id| GenericChannelId::from_str(id).ok())
            .map(|id| Cow::Owned(vec![id]));
        let channel_action = registry.register(SettingsWelcomeAction::ChannelSelect);
        let channel_kind = CreateSelectMenuKind::Channel {
            channel_types: Some(Cow::Owned(vec![ChannelType::Text])),
            default_channels,
        };

        let templates: Vec<_> = (1..=12)
            .map(|i| {
                poise::serenity_prelude::CreateSelectMenuOption::new(
                    format!("Template {i}"),
                    i.to_string(),
                )
            })
            .collect();
        let template_action = registry.register(SettingsWelcomeAction::TemplateSelect);
        let template_kind = CreateSelectMenuKind::String {
            options: templates.into(),
        };
        let template_placeholder = format!(
            "Select Template (Current: {})",
            model
                .settings
                .welcome
                .template_id
                .clone()
                .unwrap_or_else(|| "1".to_string())
        );

        let set_color_action = registry.register(SettingsWelcomeAction::SetColor(None));
        let add_message_action = if msgs < 25 {
            Some(registry.register(SettingsWelcomeAction::AddMessage(None)))
        } else {
            None
        };

        let variables_text = "### Template Variables\n> `{{ username }}` - User's display name\n> `{{ user_tag }}` - User's handle (@username)\n> `{{ server_name }}` - Server name\n> `{{ member_count }}` - Total member count\n> `{{ member_number }}` - Member join number\n> `{{ primary_color }}` - Accent color\n> `{{ welcome_message }}` - Your greetings";

        // Conditional message-removal select (only when messages exist).
        let mark_removal = if msgs > 0 {
            let options: Vec<_> = model
                .settings
                .welcome
                .messages
                .as_ref()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, msg)| {
                    let label = if model.marked_removal.contains(&i) {
                        if msg.len() > 48 {
                            format!("❌ {}...", &msg[..45])
                        } else {
                            format!("❌ {msg}")
                        }
                    } else if msg.len() > 50 {
                        format!("{}...", &msg[..47])
                    } else {
                        msg.clone()
                    };
                    let mut opt =
                        poise::serenity_prelude::CreateSelectMenuOption::new(label, i.to_string());
                    if model.marked_removal.contains(&i) {
                        opt = opt.default_selection(true);
                    }
                    opt
                })
                .collect();

            let action = registry.register(SettingsWelcomeAction::MarkRemoval);
            Some((action, options))
        } else {
            None
        };

        let save_removal_action = if msgs > 0 && !model.marked_removal.is_empty() {
            Some(registry.register(SettingsWelcomeAction::SaveRemoval))
        } else {
            None
        };
        let cancel_removal_action = if msgs > 0 && !model.marked_removal.is_empty() {
            Some(registry.register(SettingsWelcomeAction::CancelRemoval))
        } else {
            None
        };

        let back_action = registry.register(SettingsWelcomeAction::Back);
        let about_action = registry.register(SettingsWelcomeAction::About);

        let mut components: Vec<CreateContainerComponent<'_>> = vec![
            CreateContainerComponent::TextDisplay(component! {
                text_display { content: status_text }
            }),
            CreateContainerComponent::ActionRow(component! {
                action_row {
                    button {
                        custom_id: toggle_action.id,
                        label: enabled_label,
                        style: enabled_style
                    }
                }
            }),
            CreateContainerComponent::ActionRow(component! {
                action_row {
                    select_menu {
                        custom_id: channel_action.id,
                        kind: channel_kind,
                        placeholder: "Select Welcome Channel"
                    }
                }
            }),
            CreateContainerComponent::ActionRow(component! {
                action_row {
                    select_menu {
                        custom_id: template_action.id,
                        kind: template_kind,
                        placeholder: template_placeholder
                    }
                }
            }),
        ];

        // Action row: Set Color + [Add Welcome Message] + Preview Templates link.
        let mut button_row: Vec<CreateButton<'_>> =
            vec![set_color_action.as_button().style(ButtonStyle::Primary)];
        if let Some(add) = &add_message_action {
            button_row.push(add.clone().as_button().style(ButtonStyle::Primary));
        }
        button_row.push(
            CreateButton::new_link(
                "https://github.com/FAZuH/pwr-bot/blob/main/docs/welcome_templates_preview.png",
            )
            .label("Preview Templates"),
        );
        components.push(CreateContainerComponent::ActionRow(
            CreateActionRow::Buttons(button_row.into()),
        ));

        components.push(CreateContainerComponent::TextDisplay(component! {
            text_display { content: variables_text }
        }));

        if let Some((action, options)) = mark_removal {
            let kind = CreateSelectMenuKind::String {
                options: options.into(),
            };
            components.push(CreateContainerComponent::ActionRow(component! {
                action_row {
                    select_menu {
                        custom_id: action.id,
                        kind: kind,
                        min_values: 0,
                        max_values: msgs as u8,
                        placeholder: "Select messages to remove"
                    }
                }
            }));

            if let (Some(save), Some(cancel)) = (save_removal_action, cancel_removal_action) {
                components.push(CreateContainerComponent::ActionRow(component! {
                    action_row {
                        button {
                            custom_id: save.id,
                            label: save.label,
                            style: ButtonStyle::Danger
                        }
                        button {
                            custom_id: cancel.id,
                            label: cancel.label,
                            style: ButtonStyle::Secondary
                        }
                    }
                }));
            }
        }

        let container = CreateComponent::Container(CreateContainer::new(components));
        let nav_buttons = CreateComponent::ActionRow(component! {
            action_row {
                button {
                    custom_id: back_action.id,
                    label: back_action.label,
                    style: ButtonStyle::Secondary
                }
                button {
                    custom_id: about_action.id,
                    label: about_action.label,
                    style: ButtonStyle::Secondary
                }
            }
        });

        vec![container, nav_buttons]
    }

    fn translate(
        action: &Self::Action,
        values: SelectValues,
        _model: &Self::Model,
    ) -> Option<Self::Msg> {
        match action {
            SettingsWelcomeAction::ToggleEnabled => Some(WelcomeSettingsMsg::ToggleEnabled),
            SettingsWelcomeAction::ChannelSelect => {
                let channel = match values {
                    SelectValues::Channel(v) => v.first().map(|id| id.to_string()),
                    _ => None,
                };
                channel.map(|id| WelcomeSettingsMsg::SetChannel(Some(id)))
            }
            SettingsWelcomeAction::TemplateSelect => {
                let template = match values {
                    SelectValues::String(v) => v.first().cloned(),
                    _ => None,
                };
                template.map(|t| WelcomeSettingsMsg::SetTemplate(Some(t)))
            }
            // Modal submissions bypass actions entirely: the modal flow spawned
            // by `open_modal` sends `AddMessage(String)` / `SetColor(String)`
            // directly, so a bare trigger action never translates.
            SettingsWelcomeAction::SetColor(_) | SettingsWelcomeAction::AddMessage(_) => None,
            SettingsWelcomeAction::MarkRemoval => {
                let mut indices = std::collections::HashSet::new();
                if let SelectValues::String(vals) = values {
                    for val in vals {
                        if let Ok(idx) = val.parse::<usize>() {
                            indices.insert(idx);
                        }
                    }
                }
                Some(WelcomeSettingsMsg::MarkRemoval(indices))
            }
            SettingsWelcomeAction::SaveRemoval => Some(WelcomeSettingsMsg::SaveRemoval),
            SettingsWelcomeAction::CancelRemoval => Some(WelcomeSettingsMsg::CancelRemoval),
            SettingsWelcomeAction::Back => Some(WelcomeSettingsMsg::Back),
            SettingsWelcomeAction::About => Some(WelcomeSettingsMsg::About),
        }
    }

    fn attachments(model: &Self::Model) -> Vec<CreateAttachment<'static>> {
        model
            .image_bytes()
            .map(|bytes| CreateAttachment::bytes(bytes.to_vec(), WELCOME_FILE))
            .into_iter()
            .collect()
    }

    fn exit_navigation(msg: &Self::Msg) -> Option<Navigation> {
        match msg {
            WelcomeSettingsMsg::Back => Some(Navigation::SettingsMain),
            WelcomeSettingsMsg::About => Some(Navigation::SettingsAbout),
            _ => None,
        }
    }

    /// Opens the add-message / set-color modals.
    ///
    /// Modal mapping choice: the old view called `spawn_modal_component` on the
    /// button interaction and returned `ViewCmd::AlreadyResponded`, which told
    /// the engine to skip its auto-acknowledge (the modal response already
    /// answers the interaction) and to skip re-rendering. The host cannot do
    /// this through `translate`/`update` — those are pure and never see the
    /// interaction — so the modal trigger is instead an [`GuiFeature`] seam:
    /// the host consults `open_modal` before translating, and on `true` skips
    /// both the acknowledge and the render (the `AlreadyResponded` equivalent).
    /// The spawned task awaits the modal submission via
    /// `poise::execute_modal_on_component_interaction` (exactly as the old
    /// `spawn_modal_component` did) and delivers the result as a plain
    /// `AddMessage(String)` / `SetColor(String)` message on the host channel.
    /// A dismissed modal yields no message, so the view simply stays as-is —
    /// same as before.
    fn open_modal(
        action: &Self::Action,
        ctx: poise::serenity_prelude::Context,
        interaction: ComponentInteraction,
        tx: tokio::sync::mpsc::UnboundedSender<Self::Msg>,
    ) -> bool {
        match action {
            SettingsWelcomeAction::AddMessage(None) => {
                tokio::spawn(async move {
                    let modal = poise::execute_modal_on_component_interaction::<
                        AddWelcomeMessageModal,
                    >(&ctx, interaction, None, None)
                    .await;
                    if let Ok(Some(m)) = modal {
                        let _ = tx.send(WelcomeSettingsMsg::AddMessage(m.message.to_string()));
                    }
                });
                true
            }
            SettingsWelcomeAction::SetColor(None) => {
                tokio::spawn(async move {
                    let modal =
                        poise::execute_modal_on_component_interaction::<SetPrimaryColorModal>(
                            &ctx,
                            interaction,
                            None,
                            None,
                        )
                        .await;
                    if let Ok(Some(m)) = modal {
                        let _ = tx.send(WelcomeSettingsMsg::SetColor(m.color.to_string()));
                    }
                });
                true
            }
            _ => false,
        }
    }
}

/// Generates a welcome card preview given settings and generator.
pub(crate) async fn generate_preview_from(
    settings: &ServerSettings,
    generator: &WelcomeImageGenerator,
) -> Option<Vec<u8>> {
    if !settings.welcome.enabled.unwrap_or(false) {
        return None;
    }
    // Preview uses placeholder data since we don't have a real member context here
    let data = WelcomeCardData {
        template_id: settings
            .welcome
            .template_id
            .clone()
            .unwrap_or_else(|| "1".to_string()),
        username: "PreviewUser".to_string(),
        user_tag: "@previewuser".to_string(),
        avatar_url: String::new(),
        avatar_b64: None,
        server_name: "Your Server".to_string(),
        member_count: "100".to_string(),
        member_number: "#100".to_string(),
        primary_color: settings
            .welcome
            .primary_color
            .clone()
            .unwrap_or_else(|| "#5865F2".to_string()),
        welcome_message: settings
            .welcome
            .messages
            .as_ref()
            .and_then(|m| m.first())
            .cloned()
            .unwrap_or_else(|| "Welcome to the server!".to_string()),
    };
    generator.generate_card(data).await.ok()
}

/// The effect adapter that persists settings and renders the preview image.
pub struct WelcomeSettingsEffectHandler {
    service: Arc<dyn FeedSubscriptionProvider>,
    guild_id: u64,
    generator: Arc<WelcomeImageGenerator>,
}

impl WelcomeSettingsEffectHandler {
    /// Creates an adapter bound to the guild's feed subscription service and
    /// the welcome image generator.
    pub fn new(
        service: Arc<dyn FeedSubscriptionProvider>,
        guild_id: u64,
        generator: Arc<WelcomeImageGenerator>,
    ) -> Self {
        Self {
            service,
            guild_id,
            generator,
        }
    }
}

impl EffectHandler for WelcomeSettingsEffectHandler {
    type Effect = WelcomeSettingsEffect;
    type Msg = WelcomeSettingsMsg;

    fn execute(
        &mut self,
        effect: WelcomeSettingsEffect,
        tx: tokio::sync::mpsc::UnboundedSender<WelcomeSettingsMsg>,
    ) -> Vec<WelcomeSettingsMsg> {
        match effect {
            WelcomeSettingsEffect::PersistSettings(settings) => {
                let service = self.service.clone();
                let guild_id = self.guild_id;
                tokio::spawn(async move {
                    if let Err(e) = service.update_server_settings(guild_id, settings).await {
                        log::error!("welcome settings persist failed: {e}");
                    }
                    let _ = tx.send(WelcomeSettingsMsg::SettingsPersisted);
                });
                vec![]
            }
            WelcomeSettingsEffect::RenderImage(settings) => {
                let generator = self.generator.clone();
                tokio::spawn(async move {
                    let bytes = generate_preview_from(&settings, &generator).await;
                    let _ = tx.send(WelcomeSettingsMsg::ImageRendered(bytes));
                });
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::view::ActionRegistry;
    use crate::entity::WelcomeSettings;

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

    fn make_model(welcome: WelcomeSettings, marked: &[usize]) -> WelcomeSettingsModel {
        let settings = ServerSettings {
            welcome,
            ..Default::default()
        };
        let mut model = WelcomeSettingsModel::new(settings, None);
        for &i in marked {
            model.marked_removal.insert(i);
        }
        model
    }

    fn capture(model: &WelcomeSettingsModel) -> serde_json::Value {
        let mut registry = ActionRegistry::new();
        let components = WelcomeFeature::view(model, &mut registry);
        let mut value = serde_json::to_value(&components).unwrap();
        normalize_custom_ids(&mut value);
        value
    }

    #[test]
    fn welcome_render_with_msgs_snapshot() {
        let welcome = WelcomeSettings {
            enabled: Some(true),
            channel_id: Some("123456789".to_string()),
            primary_color: None,
            template_id: Some("1".to_string()),
            messages: Some(vec!["Hello!".to_string(), "Welcome!!".to_string()]),
        };
        let model = make_model(welcome, &[1]);
        let value = capture(&model);
        assert_eq!(
            value,
            serde_json::json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "-# **Settings > Welcome**\n## Welcome Settings\n\n> 🛈  Welcome cards are **active**."
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "disabled": false,
                                    "label": "Disable",
                                    "style": 4
                                }
                            ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 8,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "channel_types": [0],
                                    "default_values": [ { "id": 123456789, "type": "channel" } ],
                                    "placeholder": "Select Welcome Channel"
                                }
                            ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 3,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "options": [
                                        { "label": "Template 1", "value": "1" },
                                        { "label": "Template 2", "value": "2" },
                                        { "label": "Template 3", "value": "3" },
                                        { "label": "Template 4", "value": "4" },
                                        { "label": "Template 5", "value": "5" },
                                        { "label": "Template 6", "value": "6" },
                                        { "label": "Template 7", "value": "7" },
                                        { "label": "Template 8", "value": "8" },
                                        { "label": "Template 9", "value": "9" },
                                        { "label": "Template 10", "value": "10" },
                                        { "label": "Template 11", "value": "11" },
                                        { "label": "Template 12", "value": "12" }
                                    ],
                                    "placeholder": "Select Template (Current: 1)"
                                }
                            ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "disabled": false,
                                    "label": "Set Color",
                                    "style": 1
                                },
                                {
                                    "type": 2,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "disabled": false,
                                    "label": "Add Welcome Message",
                                    "style": 1
                                },
                                {
                                    "type": 2,
                                    "disabled": false,
                                    "label": "Preview Templates",
                                    "style": 5,
                                    "url": "https://github.com/FAZuH/pwr-bot/blob/main/docs/welcome_templates_preview.png"
                                }
                            ]
                        },
                        {
                            "type": 10,
                            "content": "### Template Variables\n> `{{ username }}` - User's display name\n> `{{ user_tag }}` - User's handle (@username)\n> `{{ server_name }}` - Server name\n> `{{ member_count }}` - Total member count\n> `{{ member_number }}` - Member join number\n> `{{ primary_color }}` - Accent color\n> `{{ welcome_message }}` - Your greetings"
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 3,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "max_values": 2,
                                    "min_values": 0,
                                    "options": [
                                        { "label": "Hello!", "value": "0" },
                                        { "default": true, "label": "❌ Welcome!!", "value": "1" }
                                    ],
                                    "placeholder": "Select messages to remove"
                                }
                            ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "disabled": false,
                                    "label": "Save Removals",
                                    "style": 4
                                },
                                {
                                    "type": 2,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "disabled": false,
                                    "label": "Cancel",
                                    "style": 2
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
                            "custom_id": "id:SettingsWelcomeAction",
                            "disabled": false,
                            "label": "❮ Back",
                            "style": 2
                        },
                        {
                            "type": 2,
                            "custom_id": "id:SettingsWelcomeAction",
                            "disabled": false,
                            "label": "🛈 About",
                            "style": 2
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn welcome_render_no_msgs_snapshot() {
        let welcome = WelcomeSettings {
            enabled: Some(true),
            channel_id: Some("123456789".to_string()),
            primary_color: None,
            template_id: Some("1".to_string()),
            messages: None,
        };
        let model = make_model(welcome, &[]);
        let value = capture(&model);
        assert_eq!(
            value,
            serde_json::json!([
                {
                    "type": 17,
                    "components": [
                        {
                            "type": 10,
                            "content": "-# **Settings > Welcome**\n## Welcome Settings\n\n> 🛈  Welcome cards are **active**."
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "disabled": false,
                                    "label": "Disable",
                                    "style": 4
                                }
                            ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 8,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "channel_types": [0],
                                    "default_values": [ { "id": 123456789, "type": "channel" } ],
                                    "placeholder": "Select Welcome Channel"
                                }
                            ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 3,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "options": [
                                        { "label": "Template 1", "value": "1" },
                                        { "label": "Template 2", "value": "2" },
                                        { "label": "Template 3", "value": "3" },
                                        { "label": "Template 4", "value": "4" },
                                        { "label": "Template 5", "value": "5" },
                                        { "label": "Template 6", "value": "6" },
                                        { "label": "Template 7", "value": "7" },
                                        { "label": "Template 8", "value": "8" },
                                        { "label": "Template 9", "value": "9" },
                                        { "label": "Template 10", "value": "10" },
                                        { "label": "Template 11", "value": "11" },
                                        { "label": "Template 12", "value": "12" }
                                    ],
                                    "placeholder": "Select Template (Current: 1)"
                                }
                            ]
                        },
                        {
                            "type": 1,
                            "components": [
                                {
                                    "type": 2,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "disabled": false,
                                    "label": "Set Color",
                                    "style": 1
                                },
                                {
                                    "type": 2,
                                    "custom_id": "id:SettingsWelcomeAction",
                                    "disabled": false,
                                    "label": "Add Welcome Message",
                                    "style": 1
                                },
                                {
                                    "type": 2,
                                    "disabled": false,
                                    "label": "Preview Templates",
                                    "style": 5,
                                    "url": "https://github.com/FAZuH/pwr-bot/blob/main/docs/welcome_templates_preview.png"
                                }
                            ]
                        },
                        {
                            "type": 10,
                            "content": "### Template Variables\n> `{{ username }}` - User's display name\n> `{{ user_tag }}` - User's handle (@username)\n> `{{ server_name }}` - Server name\n> `{{ member_count }}` - Total member count\n> `{{ member_number }}` - Member join number\n> `{{ primary_color }}` - Accent color\n> `{{ welcome_message }}` - Your greetings"
                        }
                    ]
                },
                {
                    "type": 1,
                    "components": [
                        {
                            "type": 2,
                            "custom_id": "id:SettingsWelcomeAction",
                            "disabled": false,
                            "label": "❮ Back",
                            "style": 2
                        },
                        {
                            "type": 2,
                            "custom_id": "id:SettingsWelcomeAction",
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
