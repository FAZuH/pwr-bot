use poise::CreateReply;
use serenity::all::CreateActionRow;
use serenity::all::CreateAttachment;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateMediaGallery;
use serenity::all::CreateMediaGalleryItem;
use serenity::all::CreateSelectMenu;
use serenity::all::CreateSelectMenuKind;
use serenity::all::CreateSelectMenuOption;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateUnfurledMediaItem;
use serenity::all::MessageFlags;

use crate::database::model::ServerSettings;

pub struct SettingsVoiceView;

impl SettingsVoiceView {
    pub fn create_reply(settings: &ServerSettings) -> CreateReply<'_> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(Self::create_components(settings))
    }

    pub fn create_components(settings: &ServerSettings) -> Vec<CreateComponent<'_>> {
        let is_enabled = settings.voice_tracking_enabled.unwrap_or(true);

        let status_text = format!(
            "## Voice Tracking Settings\n\n> 🛈  {}",
            if is_enabled {
                "Voice tracking is **active**."
            } else {
                "Voice tracking is **paused**."
            }
        );

        let enabled_select = CreateSelectMenu::new(
            "voice_settings_enabled",
            CreateSelectMenuKind::String {
                options: vec![
                    CreateSelectMenuOption::new("🟢 Enabled", "true").default_selection(is_enabled),
                    CreateSelectMenuOption::new("🔴 Disabled", "false")
                        .default_selection(!is_enabled),
                ]
                .into(),
            },
        )
        .placeholder("Toggle voice tracking");

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
            CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(enabled_select)),
        ]));

        vec![container]
    }
}

pub struct LeaderboardView;

impl<'a> LeaderboardView {
    pub fn create_reply(&self, user_rank: Option<u32>) -> CreateReply<'_> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(self.create_page(user_rank))
    }

    pub fn create_empty_reply() -> CreateReply<'static> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(vec![CreateComponent::Container(CreateContainer::new(
                vec![CreateContainerComponent::TextDisplay(
                    CreateTextDisplay::new(
                        "## Voice Leaderboard\n\nNo voice activity recorded yet.",
                    ),
                )],
            ))])
    }

    pub fn create_page(&self, user_rank: Option<u32>) -> Vec<CreateComponent<'a>> {
        let mut container_components: Vec<CreateContainerComponent> = Vec::new();

        let title = if let Some(rank) = user_rank {
            format!(
                "## Voice Leaderboard\n\nYou are rank **#{}** on this server",
                rank
            )
        } else {
            "## Voice Leaderboard".to_string()
        };
        container_components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(title),
        ));

        container_components.push(CreateContainerComponent::Separator(
            serenity::all::CreateSeparator::new(true),
        ));

        let media_gallery = CreateMediaGallery::new(vec![CreateMediaGalleryItem::new(
            CreateUnfurledMediaItem::new("attachment://leaderboard.png"),
        )]);
        container_components.push(CreateContainerComponent::MediaGallery(media_gallery));

        let container = CreateComponent::Container(CreateContainer::new(container_components));
        vec![container]
    }

    pub fn create_page_with_attachment(
        &self,
        user_rank: Option<u32>,
    ) -> (Vec<CreateComponent<'a>>, CreateAttachment<'a>) {
        let components = self.create_page(user_rank);
        let attachment = CreateAttachment::bytes(vec![], "leaderboard.png");
        (components, attachment)
    }
}
