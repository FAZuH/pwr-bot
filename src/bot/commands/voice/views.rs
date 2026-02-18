//! Views for voice tracking commands.

use std::time::Duration;

use poise::ChoiceParameter;
use serenity::all::ButtonStyle;
use serenity::all::ComponentInteraction;
use serenity::all::ComponentInteractionDataKind;
use serenity::all::CreateActionRow;
use serenity::all::CreateAttachment;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateMediaGallery;
use serenity::all::CreateMediaGalleryItem;
use serenity::all::CreateSelectMenuKind;
use serenity::all::CreateSelectMenuOption;
use serenity::all::CreateSeparator;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateUnfurledMediaItem;

use crate::action_enum;
use crate::action_extends;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::voice::TimeRange;
use crate::bot::commands::voice::VoiceLeaderboardTimeRange;
use crate::bot::commands::voice::controllers::LeaderboardSessionData;
use crate::bot::commands::voice::image_builder::LeaderboardPageBuilder;
use crate::bot::commands::voice::image_builder::PageGenerationResult;
use crate::bot::utils::format_duration;
use crate::bot::views::ChildViewResolver;
use crate::bot::views::InteractiveView;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::bot::views::pagination::PaginationAction;
use crate::bot::views::pagination::PaginationView;
use crate::database::model::ServerSettings;
use crate::view_core;

/// Number of leaderboard entries per page.
const LEADERBOARD_PER_PAGE: u32 = 10;

action_enum! {
    SettingsVoiceAction {
        ToggleEnabled,
        #[label = "❮ Back"]
        Back,
        #[label = "🛈 About"]
        About,
    }
}

/// Filename for the voice leaderboard image attachment.
pub const VOICE_LEADERBOARD_IMAGE_FILENAME: &str = "voice_leaderboard.jpg";

view_core! {
    timeout = Duration::from_secs(120),
    /// View for voice tracking settings.
    pub struct SettingsVoiceView<'a, SettingsVoiceAction> {
        pub settings: ServerSettings,
    }
}

impl<'a> SettingsVoiceView<'a> {
    /// Creates a new voice settings view.
    pub fn new(ctx: &'a Context<'a>, settings: ServerSettings) -> Self {
        Self {
            settings,
            core: Self::create_core(ctx),
        }
    }
}

impl<'a> ResponseView<'a> for SettingsVoiceView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let is_enabled = self.settings.voice.enabled.unwrap_or(true);

        let status_text = format!(
            "-# **Settings > Voice**\n## Voice Tracking Settings\n\n> 🛈  {}",
            if is_enabled {
                "Voice tracking is **active**."
            } else {
                "Voice tracking is **paused**."
            }
        );

        let enabled_button = self
            .register(SettingsVoiceAction::ToggleEnabled)
            .as_button()
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
                self.register(SettingsVoiceAction::Back)
                    .as_button()
                    .style(ButtonStyle::Secondary),
                self.register(SettingsVoiceAction::About)
                    .as_button()
                    .style(ButtonStyle::Secondary),
            ]
            .into(),
        ));

        vec![container, nav_buttons].into()
    }
}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, SettingsVoiceAction> for SettingsVoiceView<'a> {
    async fn handle(
        &mut self,
        action: &SettingsVoiceAction,
        _interaction: &ComponentInteraction,
    ) -> Option<SettingsVoiceAction> {
        match action {
            SettingsVoiceAction::ToggleEnabled => {
                let current = self.settings.voice.enabled.unwrap_or(true);
                self.settings.voice.enabled = Some(!current);
                Some(action.clone())
            }
            SettingsVoiceAction::Back | SettingsVoiceAction::About => Some(action.clone()),
        }
    }
}

view_core! {
    timeout = Duration::from_secs(120),
    /// View for displaying voice leaderboard with pagination.
    pub struct VoiceLeaderboardView<'a, VoiceLeaderboardAction> {
        pub leaderboard_data: LeaderboardSessionData,
        pub time_range: VoiceLeaderboardTimeRange,
        pub pagination: PaginationView<'a>,
        pub page_builder: LeaderboardPageBuilder<'a>,
        current_page_bytes: Option<Vec<u8>>,
    }
}

impl<'a> VoiceLeaderboardView<'a> {
    /// Creates a new leaderboard view.
    pub fn new(
        ctx: &'a Context<'a>,
        leaderboard_data: LeaderboardSessionData,
        time_range: VoiceLeaderboardTimeRange,
    ) -> Self {
        let pagination =
            PaginationView::new(ctx, leaderboard_data.len() as u32, LEADERBOARD_PER_PAGE);
        let page_builder = LeaderboardPageBuilder::new(ctx);
        Self {
            leaderboard_data,
            time_range,
            pagination,
            page_builder,
            current_page_bytes: None,
            core: Self::create_core(ctx),
        }
    }

    /// Calculates the slice indices for the current page.
    fn current_page_indices(&self) -> (usize, usize) {
        if self.leaderboard_data.is_empty() {
            return (0, 0);
        }
        let offset = ((self.pagination.current_page() - 1) * LEADERBOARD_PER_PAGE) as usize;
        let end = (offset + LEADERBOARD_PER_PAGE as usize).min(self.leaderboard_data.len());
        (offset, end)
    }

    /// Returns the rank offset for the current page.
    fn current_page_rank_offset(&self) -> u32 {
        (self.pagination.current_page() - 1) * LEADERBOARD_PER_PAGE
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
        let poise_ctx = self.core().ctx.poise_ctx;
        self.pagination = PaginationView::new(
            poise_ctx,
            self.leaderboard_data.len() as u32,
            LEADERBOARD_PER_PAGE,
        );
    }

    /// Sets the current page image bytes for attachment on edit.
    pub fn set_current_page_bytes(&mut self, bytes: Vec<u8>) {
        self.current_page_bytes = Some(bytes);
    }
}

impl<'a> ResponseView<'a> for VoiceLeaderboardView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        use VoiceLeaderboardAction::*;
        use VoiceLeaderboardTimeRange::*;

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

        let (since, until) = self.time_range.to_range();
        container.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(format!(
                "\n-# Time Range: **{}** — <t:{}:f> to <t:{}:R>",
                self.time_range.name(),
                since.timestamp(),
                until.timestamp(),
            )),
        ));

        container.push(CreateContainerComponent::Separator(CreateSeparator::new(
            true,
        )));

        if self.leaderboard_data.is_empty() {
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

        let time_range_menu = self
            .register(TimeRange)
            .as_select(CreateSelectMenuKind::String {
                options: vec![
                    Today.into(),
                    Past3Days.into(),
                    ThisWeek.into(),
                    Past2Weeks.into(),
                    ThisMonth.into(),
                    ThisYear.into(),
                    AllTime.into(),
                ]
                .into(),
            })
            .placeholder("Select time range");

        let action_row = CreateActionRow::SelectMenu(time_range_menu);

        let mut components = vec![
            CreateComponent::Container(CreateContainer::new(container)),
            CreateComponent::ActionRow(action_row),
        ];

        self.pagination.attach_if_multipage(&mut components);

        components.into()
    }

    fn create_reply<'b>(&mut self) -> poise::CreateReply<'b> {
        let response = self.create_response();
        let mut reply: poise::CreateReply<'b> = response.into();

        if let Some(ref bytes) = self.current_page_bytes {
            let attachment =
                CreateAttachment::bytes(bytes.clone(), VOICE_LEADERBOARD_IMAGE_FILENAME);
            reply = reply.attachment(attachment);
        }

        reply
    }
}

impl From<VoiceLeaderboardTimeRange> for CreateSelectMenuOption<'static> {
    fn from(range: VoiceLeaderboardTimeRange) -> Self {
        CreateSelectMenuOption::new(range.name(), range.name())
    }
}

action_extends! {
    VoiceLeaderboardAction extends PaginationAction {
        TimeRange,
    }
}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, VoiceLeaderboardAction> for VoiceLeaderboardView<'a> {
    async fn handle(
        &mut self,
        action: &VoiceLeaderboardAction,
        interaction: &ComponentInteraction,
    ) -> Option<VoiceLeaderboardAction> {
        use VoiceLeaderboardAction::*;
        match action {
            Base(pagination_action) => {
                let action = self
                    .pagination
                    .handle(pagination_action, interaction)
                    .await?;
                Some(VoiceLeaderboardAction::Base(action))
            }
            TimeRange => {
                if let ComponentInteractionDataKind::StringSelect { values } =
                    &interaction.data.kind
                    && let Some(time_range) = values
                        .first()
                        .and_then(|v| VoiceLeaderboardTimeRange::from_display_name(v))
                    && self.time_range != time_range
                {
                    self.time_range = time_range;
                    return Some(action.clone());
                }
                None
            }
        }
    }

    async fn on_timeout(&mut self) -> Result<(), Error> {
        self.pagination.disabled = true;
        Ok(())
    }
    fn children(&mut self) -> Vec<Box<dyn ChildViewResolver<VoiceLeaderboardAction> + '_>> {
        vec![Self::child(
            &mut self.pagination,
            VoiceLeaderboardAction::Base,
        )]
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
