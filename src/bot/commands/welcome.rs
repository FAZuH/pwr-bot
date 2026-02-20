//! Welcome commands module.

pub mod image_generator;


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
use crate::bot::views::InteractiveView;
use crate::bot::views::RenderExt;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::controller;
use crate::model::ServerSettings;
use crate::view_core;
use log::debug;
use poise::Command;
use poise::Modal;
use serenity::all::ButtonStyle;
use serenity::all::ChannelType;
use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionDataKind;
use serenity::all::CreateActionRow;
use serenity::all::CreateAttachment;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateSelectMenuKind;
use serenity::all::CreateTextDisplay;
use std::borrow::Cow;
use std::collections::HashSet;
use std::time::Duration;

const WELCOME_FILE: &str = "welcome_preview.png";

/// Server welcome configuration.
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

controller! { pub struct WelcomeSettingsController<'a> {} }

impl<'a> WelcomeSettingsController<'a> {
    async fn generate_preview(
        ctx: &Context<'_>,
        settings: &ServerSettings,
        generator: &WelcomeImageGenerator,
    ) -> Option<Vec<u8>> {
        if !settings.welcome.enabled.unwrap_or(false) {
            return None;
        }

        let author = ctx.author();
        let guild_name = ctx
            .guild()
            .map(|g| g.name.to_string())
            .unwrap_or_else(|| "Server".to_string());
        let member_count = ctx.guild().map(|g| g.member_count).unwrap_or(1);

        let data = crate::bot::commands::welcome::image_generator::WelcomeCardData {
            template_id: settings
                .welcome
                .template_id
                .clone()
                .unwrap_or_else(|| "1".to_string()),
            username: author.name.to_string(),
            user_tag: author.tag().to_string(),
            avatar_url: author.face(),
            avatar_b64: None,
            server_name: guild_name,
            member_count: member_count.to_string(),
            member_number: format!("#{}", member_count),
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

        let service = ctx.data().service.feed_subscription.clone(); // Use feed_subscription service for now since it manages ServerSettings

        let settings = service
            .get_server_settings(guild_id)
            .await
            .map_err(Error::from)?;

        let generator = WelcomeImageGenerator::new();
        let mut view = SettingsWelcomeView::new(&ctx, settings);

        view.current_image_bytes = Self::generate_preview(&ctx, &view.settings, &generator).await;

        view.render().await?;

        while let Some((action, _)) = view.listen_once().await? {
            use SettingsWelcomeAction::*;
            match action {
                Back => return Ok(NavigationResult::Back),
                About => {
                    return Ok(NavigationResult::SettingsAbout);
                }
                AddMessage(Some(modal)) => {
                    view.render().await?;

                    let msg = modal.message.trim().to_string();
                    if !msg.is_empty() {
                        let mut msgs =
                            view.settings.welcome.messages.clone().unwrap_or_default();
                        if msgs.len() < 25 {
                            msgs.push(msg);
                            view.settings.welcome.messages = Some(msgs);
                            service
                                .update_server_settings(guild_id, view.settings.clone())
                                .await?;
                            view.current_image_bytes =
                                Self::generate_preview(&ctx, &view.settings, &generator).await;

                            view.render().await?;
                        }
                    }
                }
                SetColor(Some(modal)) => {
                    view.render().await?;

                    let color = modal.color.trim().to_string();
                    if color.starts_with('#') {
                        view.settings.welcome.primary_color = Some(color);
                        service
                            .update_server_settings(guild_id, view.settings.clone())
                            .await?;
                        view.current_image_bytes =
                            Self::generate_preview(&ctx, &view.settings, &generator).await;
                        view.render().await?;
                    }
                }
                SaveRemoval => {
                    let msgs = view.settings.welcome.messages.clone().unwrap_or_default();
                    let mut new_msgs = Vec::new();
                    for (i, msg) in msgs.into_iter().enumerate() {
                        if !view.marked_removal.contains(&i) {
                            new_msgs.push(msg);
                        }
                    }
                    view.settings.welcome.messages = Some(new_msgs);
                    view.marked_removal.clear();
                    service
                        .update_server_settings(guild_id, view.settings.clone())
                        .await?;
                    view.render().await?;
                }
                CancelRemoval => {
                    view.marked_removal.clear();
                    view.render().await?;
                }
                _ => {
                    // For ToggleEnabled, ChannelSelect, TemplateSelect, MarkRemoval
                    // state is already updated in handle()
                    service
                        .update_server_settings(guild_id, view.settings.clone())
                        .await?;
                    view.render().await?;
                }
            }
        }

        Ok(NavigationResult::Exit)
    }
}

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

view_core! {
    timeout = Duration::from_secs(120),
    pub struct SettingsWelcomeView<'a, SettingsWelcomeAction> {
        pub settings: ServerSettings,
        pub marked_removal: HashSet<usize>,
        pub current_image_bytes: Option<Vec<u8>>,
    }
}

impl<'a> SettingsWelcomeView<'a> {
    pub fn new(ctx: &'a Context<'a>, settings: ServerSettings) -> Self {
        Self {
            settings,
            marked_removal: HashSet::new(),
            current_image_bytes: None,
            core: Self::create_core(ctx),
        }
    }
}

impl<'a> ResponseView<'a> for SettingsWelcomeView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let is_enabled = self.settings.welcome.enabled.unwrap_or(false);

        let status_text = format!(
            "-# **Settings > Welcome Cards**\n## Welcome Settings\n\n> 🛈  {}",
            if is_enabled {
                "Welcome cards are **active**."
            } else {
                "Welcome cards are **disabled**."
            }
        );

        let enabled_button = self
            .register(SettingsWelcomeAction::ToggleEnabled)
            .as_button()
            .label(if is_enabled { "Disable" } else { "Enable" })
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

        let channel_select = self
            .register(SettingsWelcomeAction::ChannelSelect)
            .as_select(CreateSelectMenuKind::Channel {
                channel_types: Some(Cow::Owned(vec![ChannelType::Text])),
                default_channels: None,
            })
            .placeholder("Select Welcome Channel");

        components.push(CreateContainerComponent::ActionRow(
            CreateActionRow::SelectMenu(channel_select),
        ));

        let mut templates = Vec::new();
        for i in 1..=12 {
            templates.push(serenity::all::CreateSelectMenuOption::new(
                format!("Template {}", i),
                i.to_string(),
            ));
        }
        let template_select = self
            .register(SettingsWelcomeAction::TemplateSelect)
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

        let mut button_row = Vec::new();
        button_row.push(
            self.register(SettingsWelcomeAction::SetColor(None))
                .as_button()
                .label("Set Color")
                .style(ButtonStyle::Primary),
        );

        let msgs = self
            .settings
            .welcome
            .messages
            .as_ref()
            .map(|m| m.len())
            .unwrap_or(0);
        if msgs < 25 {
            button_row.push(
                self.register(SettingsWelcomeAction::AddMessage(None))
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

        // Removal Select
        if msgs > 0 {
            let mut options = Vec::new();
            for (i, msg) in self
                .settings
                .welcome
                .messages
                .as_ref()
                .unwrap()
                .iter()
                .enumerate()
            {
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
                let mut opt = serenity::all::CreateSelectMenuOption::new(label, i.to_string());
                if self.marked_removal.contains(&i) {
                    opt = opt.default_selection(true);
                }
                options.push(opt);
            }

            let select = self
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

            if !self.marked_removal.is_empty() {
                components.push(CreateContainerComponent::ActionRow(
                    CreateActionRow::Buttons(
                        vec![
                            self.register(SettingsWelcomeAction::SaveRemoval)
                                .as_button()
                                .style(ButtonStyle::Danger),
                            self.register(SettingsWelcomeAction::CancelRemoval)
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
                self.register(SettingsWelcomeAction::Back)
                    .as_button()
                    .style(ButtonStyle::Secondary),
                self.register(SettingsWelcomeAction::About)
                    .as_button()
                    .style(ButtonStyle::Secondary),
            ]
            .into(),
        ));

        vec![container, nav_buttons].into()
    }

    fn create_reply<'b>(&mut self) -> poise::CreateReply<'b> {
        let response = self.create_response();
        let mut reply: poise::CreateReply<'b> = response.into();

        if let Some(ref bytes) = self.current_image_bytes {
            let attachment = CreateAttachment::bytes(bytes.clone(), WELCOME_FILE);
            reply = reply.attachment(attachment);
        }

        reply
    }
}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, SettingsWelcomeAction> for SettingsWelcomeView<'a> {
    async fn handle(
        &mut self,
        action: &SettingsWelcomeAction,
        interaction: &ComponentInteraction,
    ) -> Result<Option<SettingsWelcomeAction>, Error> {
        use SettingsWelcomeAction::*;
        let action = action.clone();
        match action {
            AddMessage(None) => {
                let modal_result = poise::execute_modal_on_component_interaction::<AddWelcomeMessageModal>(
                    self.core.ctx.poise_ctx.serenity_context(),
                    interaction.clone(),
                    None,
                    None,
                ).await?;
                debug!("modal_result {modal_result:?}");
                return Ok(Some(AddMessage(modal_result)));
            }
            SetColor(None) => {
                let modal_result = poise::execute_modal_on_component_interaction::<SetPrimaryColorModal>(
                    self.core.ctx.poise_ctx.serenity_context(),
                    interaction.clone(),
                    None,
                    None,
                ).await?;
                return Ok(Some(SetColor(modal_result)));
            }
            _ => {
                interaction
                    .create_response(
                        self.core.ctx.poise_ctx.http(),
                        serenity::all::CreateInteractionResponse::Acknowledge,
                    )
                    .await
                    .ok();
            }
        }

        let new_action = match action {
            ToggleEnabled => {
                let current = self.settings.welcome.enabled.unwrap_or(false);
                self.settings.welcome.enabled = Some(!current);
                action
            }
            ChannelSelect => {
                if let ComponentInteractionDataKind::ChannelSelect { values } =
                    &interaction.data.kind
                    && let Some(channel) = values.first()
                {
                    self.settings.welcome.channel_id = Some(channel.to_string());
                }
                action
            }
            TemplateSelect => {
                if let ComponentInteractionDataKind::StringSelect { values } =
                    &interaction.data.kind
                    && let Some(template) = values.first()
                {
                    self.settings.welcome.template_id = Some(template.clone());
                }
                action
            }
            MarkRemoval => {
                if let ComponentInteractionDataKind::StringSelect { values } =
                    &interaction.data.kind
                {
                    self.marked_removal.clear();
                    for val in values {
                        if let Ok(idx) = val.parse::<usize>() {
                            self.marked_removal.insert(idx);
                        }
                    }
                }
                action
            }
            _ => action,
        };
        Ok(Some(new_action.clone()))
    }

    fn should_acknowledge() -> bool {
        false
    }

}
