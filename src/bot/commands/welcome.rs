//! Welcome commands module.

pub mod image_generator;

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use log::debug;
use poise::Command;
use poise::Modal;
use poise::serenity_prelude::*;

use crate::action_enum;
use crate::bot::Data;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::settings::SettingsPage;
use crate::bot::commands::settings::run_settings;
use crate::bot::commands::welcome::image_generator::WelcomeImageGenerator;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::ActionRegistry;
use crate::bot::views::RegisteredAction;
use crate::bot::views::ResponseKind;
use crate::bot::views::Trigger;
use crate::bot::views::ViewCommand;
use crate::bot::views::ViewContext;
use crate::bot::views::ViewEngine;
use crate::bot::views::ViewHandler;
use crate::bot::views::ViewRender;
use crate::controller;
use crate::entity::ServerSettings;

const WELCOME_FILE: &str = "welcome_preview.png";

/// Configure welcome cards for new members
#[poise::command(slash_command)]
pub async fn welcome(ctx: Context<'_>) -> Result<(), Error> {
    run_settings(ctx, Some(SettingsPage::Welcome)).await
}

pub fn welcome_commands() -> Command<Data, Error> {
    welcome()
}

#[derive(Debug, Modal, Clone, PartialEq, Eq)]
#[name = "Add Welcome Message"]
pub struct AddWelcomeMessageModal {
    #[name = "Message"]
    #[placeholder = "Welcome to {{ server_name }}, {{ user_tag }}!"]
    #[paragraph]
    #[min_length = 1]
    #[max_length = 200]
    message: String,
}

#[derive(Debug, Modal, Clone, PartialEq, Eq)]
#[name = "Set Primary Color"]
pub struct SetPrimaryColorModal {
    #[name = "Primary Color (Hex)"]
    #[placeholder = "#5865F2"]
    #[min_length = 4]
    #[max_length = 7]
    color: String,
}

// ── Handler ──────────────────────────────────────────────────────────────────

pub struct SettingsWelcomeHandler {
    pub settings: ServerSettings,
    pub marked_removal: HashSet<usize>,
    pub current_image_bytes: Option<Vec<u8>>,
    service: Arc<crate::service::feed_subscription_service::FeedSubscriptionService>,
    generator: Arc<WelcomeImageGenerator>,
    guild_id: u64,
    ctx_serenity: poise::serenity_prelude::Context,
}

#[async_trait::async_trait]
impl ViewHandler<SettingsWelcomeAction> for SettingsWelcomeHandler {
    async fn handle(
        &mut self,
        action: SettingsWelcomeAction,
        trigger: Trigger<'_>,
        ctx: &ViewContext<'_, SettingsWelcomeAction>,
    ) -> Result<ViewCommand, Error> {
        use SettingsWelcomeAction::*;

        match action {
            AddMessage(None) => {
                if let Trigger::Component(interaction) = trigger {
                    let interaction = interaction.clone();
                    let ctx_serenity = self.ctx_serenity.clone();

                    ctx.spawn(async move {
                        if let Ok(Some(modal_result)) =
                            poise::execute_modal_on_component_interaction::<AddWelcomeMessageModal>(
                                &ctx_serenity,
                                interaction,
                                None,
                                None,
                            )
                            .await
                        {
                            Some(AddMessage(Some(modal_result)))
                        } else {
                            None
                        }
                    });
                }
                return Ok(ViewCommand::AlreadyResponded);
            }
            SetColor(None) => {
                if let Trigger::Component(interaction) = trigger {
                    let interaction = interaction.clone();
                    let ctx_serenity = self.ctx_serenity.clone();

                    ctx.spawn(async move {
                        if let Ok(Some(modal_result)) =
                            poise::execute_modal_on_component_interaction::<SetPrimaryColorModal>(
                                &ctx_serenity,
                                interaction,
                                None,
                                None,
                            )
                            .await
                        {
                            Some(SetColor(Some(modal_result)))
                        } else {
                            None
                        }
                    });
                }
                return Ok(ViewCommand::AlreadyResponded);
            }
            _ => {}
        }

        match action {
            ToggleEnabled => {
                let current = self.settings.welcome.enabled.unwrap_or(false);
                self.settings.welcome.enabled = Some(!current);
            }
            ChannelSelect => {
                if let Trigger::Component(interaction) = trigger
                    && let ComponentInteractionDataKind::ChannelSelect { values } =
                        &interaction.data.kind
                    && let Some(channel) = values.first()
                {
                    self.settings.welcome.channel_id = Some(channel.to_string());
                }
            }
            TemplateSelect => {
                if let Trigger::Component(interaction) = trigger
                    && let ComponentInteractionDataKind::StringSelect { values } =
                        &interaction.data.kind
                    && let Some(template) = values.first()
                {
                    self.settings.welcome.template_id = Some(template.clone());
                }
            }
            MarkRemoval => {
                if let Trigger::Component(interaction) = trigger
                    && let ComponentInteractionDataKind::StringSelect { values } =
                        &interaction.data.kind
                {
                    self.marked_removal.clear();
                    for val in values {
                        if let Ok(idx) = val.parse::<usize>() {
                            self.marked_removal.insert(idx);
                        }
                    }
                }
            }
            AddMessage(Some(ref modal)) => {
                let msg = modal.message.trim().to_string();
                if !msg.is_empty() {
                    let msgs = self.settings.welcome.messages.get_or_insert_with(Vec::new);
                    if msgs.len() < 25 {
                        msgs.push(msg);
                    }
                }
            }
            SetColor(Some(ref modal)) => {
                let color = modal.color.trim().to_string();
                if color.starts_with('#') {
                    self.settings.welcome.primary_color = Some(color);
                }
            }
            SaveRemoval => {
                let msgs = self.settings.welcome.messages.clone().unwrap_or_default();
                self.settings.welcome.messages = Some(
                    msgs.into_iter()
                        .enumerate()
                        .filter(|(i, _)| !self.marked_removal.contains(i))
                        .map(|(_, msg)| msg)
                        .collect(),
                );
                self.marked_removal.clear();
            }
            CancelRemoval => {
                self.marked_removal.clear();
            }
            Back | About => return Ok(ViewCommand::Continue),
            _ => {}
        }

        // Persist after every state-mutating action
        self.service
            .update_server_settings(self.guild_id, self.settings.clone())
            .await?;

        // Regenerate preview if welcome is enabled
        self.current_image_bytes =
            WelcomeSettingsController::generate_preview_from(&self.settings, &self.generator).await;

        Ok(ViewCommand::Render)
    }
}

// ── View ─────────────────────────────────────────────────────────────────────

impl ViewRender<SettingsWelcomeAction> for SettingsWelcomeHandler {
    fn render(&self, registry: &mut ActionRegistry<SettingsWelcomeAction>) -> ResponseKind<'_> {
        let is_enabled = self.settings.welcome.enabled.unwrap_or(false);
        let msgs = self
            .settings
            .welcome
            .messages
            .as_ref()
            .map(|m| m.len())
            .unwrap_or(0);

        let status_text = format!(
            "-# **Settings > Welcome Cards**\n## Welcome Settings\n\n> 🛈  {}",
            if is_enabled {
                "Welcome cards are **active**."
            } else {
                "Welcome cards are **disabled**."
            }
        );

        let enabled_label = if is_enabled { "Disable" } else { "Enable" };
        let enabled_action = RegisteredAction {
            id: registry.register(SettingsWelcomeAction::ToggleEnabled),
            label: "",
        };
        let enabled_button = enabled_action
            .as_button()
            .label(enabled_label)
            .style(if is_enabled {
                ButtonStyle::Danger
            } else {
                ButtonStyle::Success
            });

        let mut components = vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::Buttons(
                vec![enabled_button].into(),
            )),
        ];

        let channel_action = RegisteredAction {
            id: registry.register(SettingsWelcomeAction::ChannelSelect),
            label: "",
        };
        let channel_select = channel_action
            .as_select(CreateSelectMenuKind::Channel {
                channel_types: Some(Cow::Owned(vec![ChannelType::Text])),
                default_channels: None,
            })
            .placeholder("Select Welcome Channel");
        components.push(CreateContainerComponent::ActionRow(
            CreateActionRow::SelectMenu(channel_select),
        ));

        let templates: Vec<_> = (1..=12)
            .map(|i| {
                poise::serenity_prelude::CreateSelectMenuOption::new(
                    format!("Template {}", i),
                    i.to_string(),
                )
            })
            .collect();
        let template_action = RegisteredAction {
            id: registry.register(SettingsWelcomeAction::TemplateSelect),
            label: "",
        };
        let template_select = template_action
            .as_select(CreateSelectMenuKind::String {
                options: templates.into(),
            })
            .placeholder(format!(
                "Select Template (Current: {})",
                self.settings
                    .welcome
                    .template_id
                    .clone()
                    .unwrap_or_else(|| "1".to_string())
            ));
        components.push(CreateContainerComponent::ActionRow(
            CreateActionRow::SelectMenu(template_select),
        ));

        let color_action = RegisteredAction {
            id: registry.register(SettingsWelcomeAction::SetColor(None)),
            label: "",
        };
        let mut button_row = vec![
            color_action
                .as_button()
                .label("Set Color")
                .style(ButtonStyle::Primary),
        ];
        if msgs < 25 {
            let add_msg_action = RegisteredAction {
                id: registry.register(SettingsWelcomeAction::AddMessage(None)),
                label: "",
            };
            button_row.push(
                add_msg_action
                    .as_button()
                    .label("Add Message")
                    .style(ButtonStyle::Primary),
            );
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

        let variables_text = "### Template Variables\n> `{{ username }}` - User's display name\n> `{{ user_tag }}` - User's handle (@username)\n> `{{ server_name }}` - Server name\n> `{{ member_count }}` - Total member count\n> `{{ member_number }}` - Member join number\n> `{{ primary_color }}` - Accent color\n> `{{ welcome_message }}` - Your greetings";
        components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(variables_text),
        ));

        if msgs > 0 {
            let options: Vec<_> = self
                .settings
                .welcome
                .messages
                .as_ref()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, msg)| {
                    let label = if self.marked_removal.contains(&i) {
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
                    if self.marked_removal.contains(&i) {
                        opt = opt.default_selection(true);
                    }
                    opt
                })
                .collect();

            let mark_action = RegisteredAction {
                id: registry.register(SettingsWelcomeAction::MarkRemoval),
                label: "",
            };
            let select = mark_action
                .as_select(CreateSelectMenuKind::String {
                    options: options.into(),
                })
                .min_values(0)
                .max_values(msgs as u8)
                .placeholder("Select messages to remove");
            components.push(CreateContainerComponent::ActionRow(
                CreateActionRow::SelectMenu(select),
            ));

            if !self.marked_removal.is_empty() {
                let save_removal = RegisteredAction {
                    id: registry.register(SettingsWelcomeAction::SaveRemoval),
                    label: "",
                };
                let cancel_removal = RegisteredAction {
                    id: registry.register(SettingsWelcomeAction::CancelRemoval),
                    label: "",
                };
                components.push(CreateContainerComponent::ActionRow(
                    CreateActionRow::Buttons(
                        vec![
                            save_removal
                                .as_button()
                                .label("Save Removals")
                                .style(ButtonStyle::Danger),
                            cancel_removal
                                .as_button()
                                .label("Cancel")
                                .style(ButtonStyle::Secondary),
                        ]
                        .into(),
                    ),
                ));
            }
        }

        let container = CreateComponent::Container(CreateContainer::new(components));
        let back_action = RegisteredAction {
            id: registry.register(SettingsWelcomeAction::Back),
            label: "",
        };
        let about_action = RegisteredAction {
            id: registry.register(SettingsWelcomeAction::About),
            label: "",
        };
        let nav_buttons = CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                back_action
                    .as_button()
                    .label("❮ Back")
                    .style(ButtonStyle::Secondary),
                about_action
                    .as_button()
                    .label("🛈 About")
                    .style(ButtonStyle::Secondary),
            ]
            .into(),
        ));

        vec![container, nav_buttons].into()
    }

    fn create_reply(
        &self,
        registry: &mut ActionRegistry<SettingsWelcomeAction>,
    ) -> poise::CreateReply<'_> {
        let response = self.render(registry);
        let mut reply: poise::CreateReply<'_> = response.into();
        if let Some(ref bytes) = self.current_image_bytes {
            reply = reply.attachment(poise::serenity_prelude::CreateAttachment::bytes(
                bytes.clone(),
                WELCOME_FILE,
            ));
        }
        reply
    }
}

// ── Controller ───────────────────────────────────────────────────────────────

controller! { pub struct WelcomeSettingsController<'a> {} }

impl<'a> WelcomeSettingsController<'a> {
    /// Generates a welcome card preview given settings and generator.
    /// Extracted as a free helper so both the controller (initial render)
    /// and the handler (after each mutation) can call it.
    pub async fn generate_preview_from(
        settings: &ServerSettings,
        generator: &WelcomeImageGenerator,
    ) -> Option<Vec<u8>> {
        if !settings.welcome.enabled.unwrap_or(false) {
            return None;
        }
        // Preview uses placeholder data since we don't have a real member context here
        let data = crate::bot::commands::welcome::image_generator::WelcomeCardData {
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
}

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for WelcomeSettingsController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
        let service = ctx.data().service.feed_subscription.clone();
        let generator = Arc::new(WelcomeImageGenerator::new());

        let settings = service
            .get_server_settings(guild_id)
            .await
            .map_err(Error::from)?;

        let mut view = SettingsWelcomeHandler {
            settings,
            marked_removal: HashSet::new(),
            current_image_bytes: None,
            service,
            generator: generator.clone(),
            guild_id,
            ctx_serenity: ctx.serenity_context().clone(),
        };

        view.current_image_bytes = Self::generate_preview_from(&view.settings, &generator).await;

        let mut engine = ViewEngine::new(&ctx, view, Duration::from_secs(120));

        let nav = Arc::new(Mutex::new(NavigationResult::Exit));

        engine
            .run(|action| {
                let nav = nav.clone();
                Box::pin(async move {
                    use SettingsWelcomeAction::*;
                    debug!("on_action for {:?}", action);
                    match action {
                        Back => {
                            *nav.lock().unwrap() = NavigationResult::Back;
                            debug!("Setting nav to Back");
                            ViewCommand::Exit
                        }
                        About => {
                            *nav.lock().unwrap() = NavigationResult::SettingsAbout;
                            ViewCommand::Exit
                        }
                        _ => ViewCommand::Render,
                    }
                })
            })
            .await?;

        let nav = Arc::try_unwrap(nav).unwrap().into_inner().unwrap();
        debug!("{:?}", nav);

        Ok(nav)
    }
}

// ── Actions ───────────────────────────────────────────────────────────────────

action_enum! {
    SettingsWelcomeAction {
        ToggleEnabled,
        ChannelSelect,
        TemplateSelect,
        SetColor(Option<SetPrimaryColorModal>),
        MarkRemoval,
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
