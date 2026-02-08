use poise::CreateReply;
use serenity::all::CreateAttachment;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateMediaGallery;
use serenity::all::CreateMediaGalleryItem;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateUnfurledMediaItem;
use serenity::all::MessageFlags;

use crate::bot::views::PageNavigationView;
use crate::database::model::VoiceLeaderboardEntry;

pub struct SettingsVoiceView;

impl SettingsVoiceView {
    pub fn create_reply(settings: &crate::database::model::ServerSettings) -> CreateReply<'_> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(Self::create_components(settings))
    }

    pub fn create_components(
        settings: &crate::database::model::ServerSettings,
    ) -> Vec<CreateComponent<'_>> {
        use serenity::all::CreateActionRow;
        use serenity::all::CreateSelectMenu;
        use serenity::all::CreateSelectMenuKind;
        use serenity::all::CreateSelectMenuOption;

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

pub struct LeaderboardView<'a> {
    navigation: PageNavigationView<'a>,
}

impl<'a> LeaderboardView<'a> {
    pub fn new(navigation: PageNavigationView<'a>) -> Self {
        Self { navigation }
    }

    pub fn navigation(&mut self) -> &mut PageNavigationView<'a> {
        &mut self.navigation
    }

    pub fn create_reply(
        &self,
        entries: &[(VoiceLeaderboardEntry, String)], // (entry, display_name)
        user_rank: Option<u32>,
        image_bytes: Vec<u8>,
    ) -> CreateReply<'_> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(self.create_page(entries, user_rank, image_bytes))
    }

    pub fn create_empty_reply() -> CreateReply<'static> {
        CreateReply::new()
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(vec![CreateComponent::Container(CreateContainer::new(
                vec![CreateContainerComponent::TextDisplay(
                    CreateTextDisplay::new(
                        "## 🎙️ Voice Leaderboard\n\nNo voice activity recorded yet.",
                    ),
                )],
            ))])
    }

    pub fn create_page(
        &self,
        entries: &[(VoiceLeaderboardEntry, String)], // (entry, display_name)
        user_rank: Option<u32>,
        image_bytes: Vec<u8>,
    ) -> Vec<CreateComponent<'a>> {
        let mut container_components: Vec<CreateContainerComponent> = Vec::new();

        // Title with user's rank
        let title = if let Some(rank) = user_rank {
            format!(
                "## 🎙️ Voice Leaderboard\n\nYou are rank **#{}** on this server",
                rank
            )
        } else {
            "## 🎙️ Voice Leaderboard".to_string()
        };
        container_components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(title),
        ));

        // Separator line
        container_components.push(CreateContainerComponent::Separator(
            serenity::all::CreateSeparator::new(true),
        ));

        // Media Gallery with the leaderboard image
        let media_gallery = CreateMediaGallery::new(vec![CreateMediaGalleryItem::new(
            CreateUnfurledMediaItem::new("attachment://leaderboard.png"),
        )]);
        container_components.push(CreateContainerComponent::MediaGallery(media_gallery));

        // Create the main container
        let container = CreateComponent::Container(CreateContainer::new(container_components));

        // Add navigation buttons if multipage
        let mut components = vec![container];

        if self.navigation.pagination.pages > 1 {
            components.push(self.navigation.create_buttons());
        }

        components
    }

    pub fn create_page_with_attachment(
        &self,
        entries: &[(VoiceLeaderboardEntry, String)],
        user_rank: Option<u32>,
        image_bytes: Vec<u8>,
    ) -> (Vec<CreateComponent<'a>>, CreateAttachment<'a>) {
        let components = self.create_page(entries, user_rank, image_bytes);
        let attachment = CreateAttachment::bytes(vec![], "leaderboard.png"); // Placeholder, actual attachment passed separately
        (components, attachment)
    }
}

fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        let hours = seconds / 3600;
        let mins = (seconds % 3600) / 60;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    } else {
        let days = seconds / 86400;
        let hours = (seconds % 86400) / 3600;
        if hours > 0 {
            format!("{}d {}h", days, hours)
        } else {
            format!("{}d", days)
        }
    }
}
