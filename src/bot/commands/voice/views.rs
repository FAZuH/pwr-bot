//! Views for voice tracking commands.

use std::str::FromStr;
use std::time::Duration;

use poise::CreateReply;
use serenity::all::ButtonStyle;
use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionDataKind;
use serenity::all::CreateActionRow;
use serenity::all::CreateAttachment;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateMediaGallery;
use serenity::all::CreateMediaGalleryItem;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateUnfurledMediaItem;
use serenity::all::MessageFlags;

use crate::bot::commands::Context;
use crate::bot::views::Action;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseComponentView;
use crate::custom_id_enum;
use crate::database::model::ServerSettings;
use crate::stateful_view;

custom_id_enum!(SettingsVoiceAction {
    ToggleEnabled,
    Back = "❮ Back",
    About = "🛈 About",
});

stateful_view! {
    timeout = Duration::from_secs(120),
    /// View for voice tracking settings.
    pub struct SettingsVoiceView<'a> {
        pub settings: ServerSettings,
    }
}

impl<'a> SettingsVoiceView<'a> {
    /// Creates a new voice settings view.
    pub fn new(ctx: &'a Context<'a>, settings: ServerSettings) -> Self {
        Self {
            settings,
            ctx: Self::create_context(ctx),
        }
    }
}

impl<'a> ResponseComponentView for SettingsVoiceView<'a> {
    fn create_components<'b>(&self) -> Vec<CreateComponent<'b>> {
        let is_enabled = self.settings.voice.enabled.unwrap_or(true);

        let status_text = format!(
            "-# **Settings > Voice**\n## Voice Tracking Settings\n\n> 🛈  {}",
            if is_enabled {
                "Voice tracking is **active**."
            } else {
                "Voice tracking is **paused**."
            }
        );

        let enabled_button = CreateButton::new(SettingsVoiceAction::ToggleEnabled.custom_id())
            .label(if is_enabled { "Disable" } else { "Enable" })
            .style(if is_enabled {
                ButtonStyle::Danger
            } else {
                ButtonStyle::Success
            });

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::Buttons(
                vec![enabled_button].into(),
            )),
        ]));

        let nav_buttons = CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                CreateButton::new(SettingsVoiceAction::Back.custom_id())
                    .label(SettingsVoiceAction::Back.label())
                    .style(ButtonStyle::Secondary),
                CreateButton::new(SettingsVoiceAction::About.custom_id())
                    .label(SettingsVoiceAction::About.label())
                    .style(ButtonStyle::Secondary),
            ]
            .into(),
        ));

        vec![container, nav_buttons]
    }
}

#[async_trait::async_trait]
impl<'a> InteractableComponentView<'a, SettingsVoiceAction> for SettingsVoiceView<'a> {
    async fn handle(&mut self, interaction: &ComponentInteraction) -> Option<SettingsVoiceAction> {
        let action = SettingsVoiceAction::from_str(&interaction.data.custom_id).ok()?;

        match (&action, &interaction.data.kind) {
            (SettingsVoiceAction::ToggleEnabled, ComponentInteractionDataKind::Button) => {
                let current = self.settings.voice.enabled.unwrap_or(true);
                self.settings.voice.enabled = Some(!current);
                Some(action)
            }
            (SettingsVoiceAction::Back, _) | (SettingsVoiceAction::About, _) => Some(action),
            _ => None,
        }
    }
}

/// View that displays the voice leaderboard.
pub struct VoiceLeaderboardView {
    pub user_rank: Option<u32>,
}

impl VoiceLeaderboardView {
    /// Creates a new leaderboard view with optional user rank.
    pub fn new(user_rank: Option<u32>) -> Self {
        Self { user_rank }
    }

    /// Creates an empty leaderboard reply.
    pub fn create_empty_reply() -> CreateReply<'static> {
        CreateReply::new()
            .content("No voice activity recorded yet. Join a voice channel to start tracking!")
            .flags(MessageFlags::EPHEMERAL)
    }
}

impl ResponseComponentView for VoiceLeaderboardView {
    fn create_components<'a>(&self) -> Vec<CreateComponent<'a>> {
        let mut container = vec![CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new("### Voice Leaderboard"),
        )];

        if let Some(rank) = self.user_rank {
            container.push(CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new(format!("\n> Your current rank: **#{}**", rank)),
            ));
        }

        // container.push(CreateContainerComponent::TextDisplay(
        //     CreateTextDisplay::new(
        //         "\nVoice activity is being tracked. Use `/voice stats` to see detailed statistics.",
        //     ),
        // ));

        container.push(CreateContainerComponent::MediaGallery(
            CreateMediaGallery::new(vec![CreateMediaGalleryItem::new(
                CreateUnfurledMediaItem::new("attachment://voice_leaderboard.jpg"),
            )]),
        ));

        vec![CreateComponent::Container(CreateContainer::new(container))]
    }
}

/// Extension trait for creating leaderboard replies with attachments.
pub trait VoiceLeaderboardReply {
    /// Creates a reply with the leaderboard image attachment.
    fn create_leaderboard_reply(
        &self,
        image_data: Vec<u8>,
    ) -> impl std::future::Future<Output = CreateReply<'static>> + Send;
}

impl VoiceLeaderboardReply for VoiceLeaderboardView {
    async fn create_leaderboard_reply(&self, image_data: Vec<u8>) -> CreateReply<'static> {
        let attachment = CreateAttachment::bytes(image_data, "voice_leaderboard.jpg");

        CreateReply::new()
            .attachment(attachment)
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(self.create_components())
    }
}
