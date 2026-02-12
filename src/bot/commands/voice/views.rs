//! Views for voice tracking commands.

use std::str::FromStr;
use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionDataKind;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateMediaGallery;
use serenity::all::CreateMediaGalleryItem;
use serenity::all::CreateSelectMenu;
use serenity::all::CreateSelectMenuKind;
use serenity::all::CreateSelectMenuOption;
use serenity::all::CreateSeparator;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateUnfurledMediaItem;

use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::voice::VoiceLeaderboardTimeRange;
use crate::bot::commands::voice::controllers::LeaderboardSessionData;
use crate::bot::commands::voice::image_builder::LeaderboardPageBuilder;
use crate::bot::commands::voice::image_builder::PageGenerationResult;
use crate::bot::utils::format_duration;
use crate::bot::views::Action;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseComponentView;
use crate::bot::views::pagination::PaginationAction;
use crate::bot::views::pagination::PaginationView;
use crate::custom_id_enum;
use crate::custom_id_extends;
use crate::database::model::ServerSettings;
use crate::stateful_view;

/// Number of leaderboard entries per page.
const LEADERBOARD_PER_PAGE: u32 = 10;

custom_id_enum!(SettingsVoiceAction {
    ToggleEnabled,
    Back = "❮ Back",
    About = "🛈 About",
});

/// Filename for the voice leaderboard image attachment.
pub const VOICE_LEADERBOARD_IMAGE_FILENAME: &str = "voice_leaderboard.jpg";

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

stateful_view! {
    timeout = Duration::from_secs(120),
    pub struct VoiceLeaderboardView<'a> {
        pub leaderboard_data: LeaderboardSessionData,
        pub time_range: VoiceLeaderboardTimeRange,
        pub pagination: PaginationView<'a>,
        pub page_builder: LeaderboardPageBuilder<'a>,
    }
}

custom_id_extends! { VoiceLeaderboardAction extends PaginationAction { 
    TimeRange
} }

impl<'a> VoiceLeaderboardView<'a> {
    /// Creates a new leaderboard view.
    pub fn new(
        ctx: &'a Context<'a>,
        leaderboard_data: LeaderboardSessionData,
        time_range: VoiceLeaderboardTimeRange,
    ) -> Self {
        let pagination = PaginationView::new(ctx, leaderboard_data.len() as u32, LEADERBOARD_PER_PAGE);
        let page_builder = LeaderboardPageBuilder::new(ctx);
        Self {
            leaderboard_data,
            time_range,
            ctx: Self::create_context(ctx),
            pagination,
            page_builder
        }
    }

    /// Calculates the slice indices for the current page.
    fn current_page_indices(&self) -> (usize, usize) {
        if self.leaderboard_data.is_empty() {
            return (0, 0);
        }
        let offset = ((self.pagination.state.current_page - 1) * LEADERBOARD_PER_PAGE) as usize;
        let end = (offset + LEADERBOARD_PER_PAGE as usize).min(self.leaderboard_data.len());
        (offset, end)
    }

    /// Returns the rank offset for the current page.
    fn current_page_rank_offset(&self) -> u32 {
        (self.pagination.state.current_page - 1) * LEADERBOARD_PER_PAGE
    }

    /// Generates the page image for the current page.
    pub async fn generate_current_page(&mut self) -> Result<PageGenerationResult, Error> {
        let (offset, end) = self.current_page_indices();
        let entries = &self.leaderboard_data.entries[offset..end];
        let rank_offset = self.current_page_rank_offset();
        self.page_builder.build_page(entries, rank_offset).await
    }

    /// Updates the leaderboard data and resets pagination to page 1.
    pub fn update_leaderboard_data(&mut self, data: LeaderboardSessionData) {
        self.leaderboard_data = data;
        self.pagination = PaginationView::new(
            self.ctx.poise_ctx,
            self.leaderboard_data.len() as u32,
            LEADERBOARD_PER_PAGE,
        );
    }
}

impl ResponseComponentView for VoiceLeaderboardView<'_> {

    fn create_components<'a>(&self) -> Vec<CreateComponent<'a>> {
        let mut container = vec![CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new("### Voice Leaderboard"),
        )];

        if let Some(rank) = self.leaderboard_data.user_rank {
            let duration_text = self
                .leaderboard_data
                .user_duration
                .map(format_duration)
                .unwrap_or_else(|| "unknown".to_string());

            container.push(CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new(format!(
                    "\nYou are ranked **#{}** on this server with **{}** of voice activity.",
                    rank, duration_text
                )),
            ));
        } else {
            container.push(CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new("\nYou are not on the leaderboard for this time range."),
            ));
        }

        container.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(format!("\n-# Time Range: **{}**", self.time_range.name())),
        ));

        container.push(CreateContainerComponent::Separator(CreateSeparator::new(
            true,
        )));

        if self.leaderboard_data.is_empty(){
            container.push(CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new(
                    "No voice activity recorded yet at this time range.\n\nJoin a **voice channel** to start tracking!",
                ),
            ));
        } else {
            container.push(CreateContainerComponent::MediaGallery(
                CreateMediaGallery::new(vec![CreateMediaGalleryItem::new(
                    CreateUnfurledMediaItem::new(format!(
                        "attachment://{}",
                        VOICE_LEADERBOARD_IMAGE_FILENAME
                    )),
                )]),
            ));
        }

        let time_range_menu = CreateSelectMenu::new(
            VoiceLeaderboardAction::TimeRange.custom_id(),
            CreateSelectMenuKind::String {
                options: vec![
                    CreateSelectMenuOption::new("Today", VoiceLeaderboardTimeRange::Today.name()),
                    CreateSelectMenuOption::new(
                        "Past 3 days",
                        VoiceLeaderboardTimeRange::Past3Days.name(),
                    ),
                    CreateSelectMenuOption::new(
                        "This week",
                        VoiceLeaderboardTimeRange::ThisWeek.name(),
                    ),
                    CreateSelectMenuOption::new(
                        "Past 2 weeks",
                        VoiceLeaderboardTimeRange::Past2Weeks.name(),
                    ),
                    CreateSelectMenuOption::new(
                        "This month",
                        VoiceLeaderboardTimeRange::ThisMonth.name(),
                    ),
                    CreateSelectMenuOption::new(
                        "This year",
                        VoiceLeaderboardTimeRange::ThisYear.name(),
                    ),
                    CreateSelectMenuOption::new(
                        "All time",
                        VoiceLeaderboardTimeRange::AllTime.name(),
                    ),
                ]
                .into(),
            },
        )
        .placeholder("Select time range");

        let action_row = CreateActionRow::SelectMenu(time_range_menu);

        let mut components = vec![
            CreateComponent::Container(CreateContainer::new(container)),
            CreateComponent::ActionRow(action_row),
        ];

        self.pagination.attach_if_multipage(&mut components);

        components
    }
}

#[async_trait::async_trait]
impl<'a> InteractableComponentView<'a, VoiceLeaderboardAction> for VoiceLeaderboardView<'a> {
    async fn handle(
        &mut self,
        interaction: &ComponentInteraction,
    ) -> Option<VoiceLeaderboardAction> {
        let action = Self::get_action(interaction)?;

        match (action, &interaction.data.kind) {
            (
                VoiceLeaderboardAction::TimeRange,
                ComponentInteractionDataKind::StringSelect { values },
            ) => {

                if let Some(time_range) = values.iter().flat_map(|val| VoiceLeaderboardTimeRange::from_name(val)).next()
                    && self.time_range != time_range {
                        self.time_range = time_range;
                        return Some(VoiceLeaderboardAction::TimeRange)
                    }
                None
            },
            (VoiceLeaderboardAction::Base(pagination_action), _) => {
                Some(VoiceLeaderboardAction::Base(self.pagination.handle_action(pagination_action).await?))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voice_leaderboard_time_range_to_range() {
        // Test that to_range returns valid datetime range
        let (since, until) = VoiceLeaderboardTimeRange::Today.to_range();
        assert!(since <= until);

        let (since, until) = VoiceLeaderboardTimeRange::AllTime.to_range();
        assert_eq!(since, chrono::DateTime::UNIX_EPOCH);
        assert!(since <= until);
    }

    #[test]
    fn test_voice_leaderboard_time_range_into_datetime() {
        let now = chrono::Utc::now();
        let today_start: chrono::DateTime<chrono::Utc> = VoiceLeaderboardTimeRange::Today.into();
        assert!(today_start <= now);

        let all_time_start: chrono::DateTime<chrono::Utc> =
            VoiceLeaderboardTimeRange::AllTime.into();
        assert_eq!(all_time_start, chrono::DateTime::UNIX_EPOCH);
    }

    #[test]
    fn test_voice_leaderboard_time_range_equality() {
        let range1 = VoiceLeaderboardTimeRange::Today;
        let range2 = VoiceLeaderboardTimeRange::Today;
        let range3 = VoiceLeaderboardTimeRange::ThisMonth;

        assert_eq!(range1, range2);
        assert_ne!(range1, range3);
    }
}
