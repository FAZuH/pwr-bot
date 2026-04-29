//! Welcome commands module.
use std::borrow::Cow;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::bot::command::prelude::*;
use crate::bot::command::welcome::image_generator::WelcomeCardData;
use crate::bot::command::welcome::image_generator::WelcomeImageGenerator;
use crate::entity::ServerSettings;
use crate::service::traits::FeedSubscriptionProvider;
use crate::update::Update;
use crate::update::welcome_settings::WelcomeSettingsCmd;
use crate::update::welcome_settings::WelcomeSettingsModel;
use crate::update::welcome_settings::WelcomeSettingsMsg;
use crate::update::welcome_settings::WelcomeSettingsUpdate;

pub mod image_generator;

const WELCOME_FILE: &str = "welcome_preview.png";

/// Configure welcome cards for new members
#[poise::command(slash_command)]
pub async fn welcome(ctx: Context<'_>) -> Result<(), Error> {
    Coordinator::new(ctx)
        .run(NavigationResult::SettingsWelcome)
        .await?;
    Ok(())
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
    pub model: WelcomeSettingsModel,
    pub settings: ServerSettings,
    pub current_image_bytes: Option<Vec<u8>>,
    pub(crate) service: Arc<dyn FeedSubscriptionProvider>,
    pub(crate) generator: Arc<WelcomeImageGenerator>,
    pub(crate) guild_id: u64,
    pub(crate) ctx_serenity: poise::serenity_prelude::Context,
}

impl SettingsWelcomeHandler {
    async fn persist_and_regenerate(&mut self) -> Result<(), Error> {
        self.settings.welcome = self.model.settings.clone();
        self.service
            .update_server_settings(self.guild_id, self.settings.clone())
            .await?;
        self.current_image_bytes =
            WelcomeSettingsController::generate_preview_from(&self.settings, &self.generator).await;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ViewHandler for SettingsWelcomeHandler {
    type Action = SettingsWelcomeAction;
    async fn handle(
        &mut self,
        ctx: ViewContext<'_, SettingsWelcomeAction>,
    ) -> Result<ViewCommand, Error> {
        use SettingsWelcomeAction::*;

        let action = ctx.action();
        match action {
            SetColor(None) => {
                if let ViewEvent::Component(ref interaction) = ctx.event {
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
            AddMessage(None) => {
                if let ViewEvent::Component(ref interaction) = ctx.event {
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
            ToggleEnabled => {
                let cmd = WelcomeSettingsUpdate::update(
                    WelcomeSettingsMsg::ToggleEnabled,
                    &mut self.model,
                );
                if matches!(cmd, WelcomeSettingsCmd::PersistSettings) {
                    self.persist_and_regenerate().await?;
                }
            }
            ChannelSelect => {
                if let Some(channel) = ctx.channel_select_values().and_then(|v| v.first().copied())
                {
                    let cmd = WelcomeSettingsUpdate::update(
                        WelcomeSettingsMsg::SetChannel(Some(channel.to_string())),
                        &mut self.model,
                    );
                    if matches!(cmd, WelcomeSettingsCmd::PersistSettings) {
                        self.persist_and_regenerate().await?;
                    }
                }
            }
            TemplateSelect => {
                if let Some(template) = ctx.string_select_values().and_then(|v| v.first().cloned())
                {
                    let cmd = WelcomeSettingsUpdate::update(
                        WelcomeSettingsMsg::SetTemplate(Some(template)),
                        &mut self.model,
                    );
                    if matches!(cmd, WelcomeSettingsCmd::PersistSettings) {
                        self.persist_and_regenerate().await?;
                    }
                }
            }
            MarkRemoval => {
                let mut indices = HashSet::new();
                if let Some(values) = ctx.string_select_values() {
                    for val in values {
                        if let Ok(idx) = val.parse::<usize>() {
                            indices.insert(idx);
                        }
                    }
                }
                WelcomeSettingsUpdate::update(
                    WelcomeSettingsMsg::MarkRemoval(indices),
                    &mut self.model,
                );
            }
            AddMessage(Some(modal)) => {
                let cmd = WelcomeSettingsUpdate::update(
                    WelcomeSettingsMsg::AddMessage(modal.message.clone()),
                    &mut self.model,
                );
                if matches!(cmd, WelcomeSettingsCmd::PersistSettings) {
                    self.persist_and_regenerate().await?;
                }
            }
            SetColor(Some(modal)) => {
                let cmd = WelcomeSettingsUpdate::update(
                    WelcomeSettingsMsg::SetColor(modal.color.clone()),
                    &mut self.model,
                );
                if matches!(cmd, WelcomeSettingsCmd::PersistSettings) {
                    self.persist_and_regenerate().await?;
                }
            }
            SaveRemoval => {
                let cmd =
                    WelcomeSettingsUpdate::update(WelcomeSettingsMsg::SaveRemoval, &mut self.model);
                if matches!(cmd, WelcomeSettingsCmd::PersistSettings) {
                    self.persist_and_regenerate().await?;
                }
            }
            CancelRemoval => {
                WelcomeSettingsUpdate::update(WelcomeSettingsMsg::CancelRemoval, &mut self.model);
            }
            About => {
                ctx.coordinator.navigate(NavigationResult::SettingsAbout);
                return Ok(ViewCommand::Exit);
            }
            Back => {
                ctx.coordinator.navigate(NavigationResult::SettingsMain);
                return Ok(ViewCommand::Exit);
            }
        }

        Ok(ViewCommand::Render)
    }
}

// ── View ─────────────────────────────────────────────────────────────────────

impl ViewRender for SettingsWelcomeHandler {
    type Action = SettingsWelcomeAction;
    fn render(&self, registry: &mut ActionRegistry<SettingsWelcomeAction>) -> ResponseKind<'_> {
        let is_enabled = self.model.is_enabled();
        let msgs = self.model.message_count();

        let status_text = format!(
            "-# **Settings > Welcome**\n## Welcome Settings\n\n> 🛈  {}",
            if is_enabled {
                "Welcome cards are **active**."
            } else {
                "Welcome cards are **disabled**."
            }
        );

        let enabled_label = if is_enabled { "Disable" } else { "Enable" };
        let enabled_button = registry
            .register(SettingsWelcomeAction::ToggleEnabled)
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

        let default_channels = self
            .model
            .settings
            .channel_id
            .as_deref()
            .and_then(|id| GenericChannelId::from_str(id).ok())
            .map(|id| Cow::Owned(vec![id]));
        let channel_select = registry
            .register(SettingsWelcomeAction::ChannelSelect)
            .as_select(CreateSelectMenuKind::Channel {
                channel_types: Some(Cow::Owned(vec![ChannelType::Text])),
                default_channels,
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
        let template_select = registry
            .register(SettingsWelcomeAction::TemplateSelect)
            .as_select(CreateSelectMenuKind::String {
                options: templates.into(),
            })
            .placeholder(format!(
                "Select Template (Current: {})",
                self.model
                    .settings
                    .template_id
                    .clone()
                    .unwrap_or_else(|| "1".to_string())
            ));
        components.push(CreateContainerComponent::ActionRow(
            CreateActionRow::SelectMenu(template_select),
        ));

        let mut button_row = vec![
            registry
                .register(SettingsWelcomeAction::SetColor(None))
                .as_button()
                .style(ButtonStyle::Primary),
        ];
        if msgs < 25 {
            button_row.push(
                registry
                    .register(SettingsWelcomeAction::AddMessage(None))
                    .as_button()
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
                .model
                .settings
                .messages
                .as_ref()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, msg)| {
                    let label = if self.model.marked_removal.contains(&i) {
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
                    if self.model.marked_removal.contains(&i) {
                        opt = opt.default_selection(true);
                    }
                    opt
                })
                .collect();

            let select = registry
                .register(SettingsWelcomeAction::MarkRemoval)
                .as_select(CreateSelectMenuKind::String {
                    options: options.into(),
                })
                .min_values(0)
                .max_values(msgs as u8)
                .placeholder("Select messages to remove");
            components.push(CreateContainerComponent::ActionRow(
                CreateActionRow::SelectMenu(select),
            ));

            if !self.model.marked_removal.is_empty() {
                components.push(CreateContainerComponent::ActionRow(
                    CreateActionRow::Buttons(
                        vec![
                            registry
                                .register(SettingsWelcomeAction::SaveRemoval)
                                .as_button()
                                .style(ButtonStyle::Danger),
                            registry
                                .register(SettingsWelcomeAction::CancelRemoval)
                                .as_button()
                                .style(ButtonStyle::Secondary),
                        ]
                        .into(),
                    ),
                ));
            }
        }

        let container = CreateComponent::Container(CreateContainer::new(components));
        let nav_buttons = CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                registry
                    .register(SettingsWelcomeAction::Back)
                    .as_button()
                    .style(ButtonStyle::Secondary),
                registry
                    .register(SettingsWelcomeAction::About)
                    .as_button()
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
}

#[async_trait::async_trait]
impl Controller for WelcomeSettingsController<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Coordinator<'_>>) -> Result<(), Error> {
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
            model: WelcomeSettingsModel::new(settings.welcome.clone()),
            settings,
            current_image_bytes: None,
            service,
            generator: generator.clone(),
            guild_id,
            ctx_serenity: ctx.serenity_context().clone(),
        };

        view.current_image_bytes = Self::generate_preview_from(&view.settings, &generator).await;

        let mut engine = ViewEngine::new(ctx, view, Duration::from_secs(120), coordinator.clone());

        engine.run().await?;

        Ok(())
    }
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
