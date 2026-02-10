use std::str::FromStr;

use poise::CreateReply;
use serenity::all::ComponentInteraction;
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

use crate::bot::views::Action;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseComponentView;
use crate::bot::views::ViewProvider;
use crate::custom_id_enum;
use crate::database::model::ServerSettings;

custom_id_enum!(SettingsAction { EnabledSelect });

pub struct SettingsVoiceView {
    pub settings: ServerSettings,
}

impl SettingsVoiceView {
    pub fn new(settings: ServerSettings) -> Self {
        Self { settings }
    }
}

impl<'a> ViewProvider<'a> for SettingsVoiceView {
    fn create(&self) -> Vec<CreateComponent<'a>> {
        let is_enabled = self.settings.voice_tracking_enabled.unwrap_or(true);

        let status_text = format!(
            "## Voice Tracking Settings\n\n> 🛈  {}",
            if is_enabled {
                "Voice tracking is **active**."
            } else {
                "Voice tracking is **paused**."
            }
        );

        let enabled_select = CreateSelectMenu::new(
            SettingsAction::EnabledSelect.as_str(),
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

impl ResponseComponentView for SettingsVoiceView {}

#[async_trait::async_trait]
impl InteractableComponentView<SettingsAction> for SettingsVoiceView {
    async fn handle(&mut self, interaction: &ComponentInteraction) -> Option<SettingsAction> {
        use serenity::all::ComponentInteractionDataKind;

        let action = SettingsAction::from_str(&interaction.data.custom_id).ok()?;

        match (&action, &interaction.data.kind) {
            (
                SettingsAction::EnabledSelect,
                ComponentInteractionDataKind::StringSelect { values },
            ) => {
                if let Some(value) = values.first() {
                    self.settings.voice_tracking_enabled = Some(value == "true");
                }
                Some(action)
            }
            _ => None,
        }
    }
}

pub struct LeaderboardView {
    pub user_rank: Option<u32>,
}

impl LeaderboardView {
    pub fn new(user_rank: Option<u32>) -> Self {
        Self { user_rank }
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

    pub fn create_page_with_attachment<'a>(
        &'a self,
    ) -> (Vec<CreateComponent<'a>>, CreateAttachment<'a>) {
        let components = self.create();
        let attachment = CreateAttachment::bytes(vec![], "leaderboard.png");
        (components, attachment)
    }
}

impl<'a> ViewProvider<'a> for LeaderboardView {
    fn create(&self) -> Vec<CreateComponent<'a>> {
        let mut container_components: Vec<CreateContainerComponent> = Vec::new();

        let title = if let Some(rank) = self.user_rank {
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
}
