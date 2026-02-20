//! Voice leaderboard subcommand.

use std::ops::Deref;
use std::time::Duration;
use std::time::Instant;

use log::trace;
use poise::ChoiceParameter;
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

use crate::action_extends;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::settings::SettingsPage;
use crate::bot::commands::settings::run_settings;
use crate::bot::commands::voice::TimeRange;
use crate::bot::commands::voice::VoiceLeaderboardTimeRange;
use crate::bot::commands::voice::leaderboard::image_builder::LeaderboardPageBuilder;
use crate::bot::commands::voice::leaderboard::image_builder::PageGenerationResult;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::error::BotError;
use crate::bot::navigation::NavigationResult;
use crate::bot::utils::format_duration;
use crate::bot::views::ChildViewResolver;
use crate::bot::views::InteractiveView;
use crate::bot::views::RenderExt;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::bot::views::pagination::PaginationAction;
use crate::bot::views::pagination::PaginationView;
use crate::error::AppError;
use crate::model::VoiceLeaderboardEntry;
use crate::model::VoiceLeaderboardOptBuilder;
use crate::view_core;

pub mod image_builder;
pub mod image_generator;

/// Filename for the voice leaderboard image attachment.
pub const VOICE_LEADERBOARD_IMAGE_FILENAME: &str = "voice_leaderboard.jpg";

/// Number of leaderboard entries per page.
const LEADERBOARD_PER_PAGE: u32 = 10;

/// Display the voice activity leaderboard
///
/// Shows a ranked list of users by total time spent in voice channels.
/// Includes your current rank position.
#[poise::command(slash_command)]
pub async fn leaderboard(
    ctx: Context<'_>,
    #[description = "Time period to filter voice activity. Defaults to \"This month\""]
    time_range: Option<VoiceLeaderboardTimeRange>,
) -> Result<(), Error> {
    command(
        ctx,
        time_range.unwrap_or(VoiceLeaderboardTimeRange::ThisMonth),
    )
    .await
}

pub async fn command(ctx: Context<'_>, time_range: VoiceLeaderboardTimeRange) -> Result<(), Error> {
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = VoiceLeaderboardController::new(&ctx, time_range);
    let _result = controller.run(&mut coordinator).await?;
    Ok(())
}

/// Data for a leaderboard session.
pub struct LeaderboardSessionData {
    pub entries: Vec<VoiceLeaderboardEntry>,
    pub user_rank: Option<u32>,
    pub user_duration: Option<i64>,
}

impl LeaderboardSessionData {
    /// Creates session data from entries and calculates user rank.
    pub fn from_entries(entries: Vec<VoiceLeaderboardEntry>, author_id: u64) -> Self {
        let user_rank = entries
            .iter()
            .position(|e| e.user_id == author_id)
            .map(|p| p as u32 + 1);
        let user_duration = entries
            .iter()
            .find(|e| e.user_id == author_id)
            .map(|e| e.total_duration);

        Self {
            entries,
            user_rank,
            user_duration,
        }
    }
}

impl Deref for LeaderboardSessionData {
    type Target = Vec<VoiceLeaderboardEntry>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

/// Controller for voice leaderboard display and interaction.
pub struct VoiceLeaderboardController<'a> {
    #[allow(dead_code)]
    ctx: &'a Context<'a>,
    pub time_range: VoiceLeaderboardTimeRange,
}

impl<'a> VoiceLeaderboardController<'a> {
    /// Creates a new leaderboard controller.
    pub fn new(ctx: &'a Context<'a>, time_range: VoiceLeaderboardTimeRange) -> Self {
        Self { ctx, time_range }
    }

    /// Fetches leaderboard entries for the current time range.
    async fn fetch_entries(
        ctx: &Context<'_>,
        time_range: VoiceLeaderboardTimeRange,
        is_partner_mode: bool,
        target_user: Option<serenity::all::UserId>,
    ) -> Result<LeaderboardSessionData, Error> {
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
        let (since, until) = time_range.to_range();

        let voice_lb_opts = VoiceLeaderboardOptBuilder::default()
            .guild_id(guild_id)
            .limit(Some(u32::MAX))
            .since(Some(since))
            .until(Some(until))
            .build()
            .map_err(AppError::from)?;

        let new_entries = if is_partner_mode {
            let target_id = target_user.unwrap_or(ctx.author().id).get();
            ctx.data()
                .service
                .voice_tracking
                .get_partner_leaderboard(&voice_lb_opts, target_id)
                .await
                .map_err(Error::from)?
        } else {
            ctx.data()
                .service
                .voice_tracking
                .get_leaderboard_withopt(&voice_lb_opts)
                .await
                .map_err(Error::from)?
        };

        let author_id = ctx.author().id.get();
        Ok(LeaderboardSessionData::from_entries(new_entries, author_id))
    }
}

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for VoiceLeaderboardController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let controller_start = Instant::now();

        let ctx = *coordinator.context();
        ctx.defer().await?;

        // Fetch initial entries
        let session_data = Self::fetch_entries(&ctx, self.time_range, false, None).await?;

        let mut view = VoiceLeaderboardView::new(&ctx, session_data, self.time_range);

        if view.leaderboard_data.is_empty() {
            view.render().await?;
            return Ok(NavigationResult::Exit);
        }

        // Generate and send initial page
        let page_result = view.generate_current_page().await?;
        view.set_current_page_bytes(page_result.image_bytes.clone());
        view.render().await?;

        trace!(
            "controller_initial_response {} ms",
            controller_start.elapsed().as_millis()
        );

        while let Some((action, interaction)) = view.listen_once().await? {
            if matches!(action, VoiceLeaderboardAction::TimeRange)
                || matches!(action, VoiceLeaderboardAction::ToggleMode)
                || matches!(action, VoiceLeaderboardAction::SelectUser)
            {
                if matches!(action, VoiceLeaderboardAction::ToggleMode) {
                    view.is_partner_mode = !view.is_partner_mode;
                    if view.is_partner_mode && view.target_user.is_none() {
                        view.target_user = Some(ctx.author().clone());
                    }
                } else if matches!(action, VoiceLeaderboardAction::SelectUser)
                    && let ComponentInteractionDataKind::UserSelect { values } =
                        &interaction.data.kind
                    && let Some(user_id) = values.first()
                    && let Ok(user) = user_id.to_user(ctx.http()).await
                {
                    view.target_user = Some(user);
                }

                let new_data = Self::fetch_entries(
                    &ctx,
                    view.time_range,
                    view.is_partner_mode,
                    view.target_user.as_ref().map(|u| u.id),
                )
                .await?;
                view.update_leaderboard_data(new_data);
            }
            let page_result = view.generate_current_page().await?;
            view.set_current_page_bytes(page_result.image_bytes.clone());
            view.render().await?;
        }

        trace!(
            "controller_total {} ms",
            controller_start.elapsed().as_millis()
        );
        Ok(NavigationResult::Exit)
    }
}

/// Legacy function for voice settings command.
pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
    run_settings(ctx, Some(SettingsPage::Voice)).await
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
        pub is_partner_mode: bool,
        pub target_user: Option<serenity::all::User>,
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
            is_partner_mode: false,
            target_user: None,
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
            CreateTextDisplay::new(if self.is_partner_mode {
                let display_name = self
                    .target_user
                    .as_ref()
                    .map(|u| u.name.to_string())
                    .unwrap_or_else(|| "Your".to_string());
                format!("### {} Voice Partners", display_name)
            } else {
                "### Voice Leaderboard".to_string()
            }),
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

        let toggle_label = if self.is_partner_mode {
            "Show Server Leaderboard"
        } else {
            "Show Voice Partners"
        };
        let toggle_button = self
            .register(ToggleMode)
            .as_button()
            .label(toggle_label)
            .style(serenity::all::ButtonStyle::Primary);

        container.push(CreateContainerComponent::ActionRow(
            CreateActionRow::Buttons(vec![toggle_button].into()),
        ));

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

        if self.is_partner_mode {
            let default_users = self
                .target_user
                .clone()
                .map(|u| std::borrow::Cow::Owned(vec![u.id]));
            let user_select = self
                .register(SelectUser)
                .as_select(CreateSelectMenuKind::User { default_users })
                .placeholder("Select a user to view their voice partners");
            components.push(CreateComponent::ActionRow(CreateActionRow::SelectMenu(
                user_select,
            )));
        }

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
        ToggleMode,
        SelectUser,
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
            ToggleMode => Some(action.clone()),
            SelectUser => Some(action.clone()),
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
    fn test_leaderboard_session_data_from_entries() {
        let entries = vec![
            VoiceLeaderboardEntry {
                user_id: 100,
                total_duration: 3600,
            },
            VoiceLeaderboardEntry {
                user_id: 200,
                total_duration: 1800,
            },
            VoiceLeaderboardEntry {
                user_id: 300,
                total_duration: 900,
            },
        ];

        // Test author is ranked #2
        let session = LeaderboardSessionData::from_entries(entries.clone(), 200);
        assert_eq!(session.user_rank, Some(2));
        assert_eq!(session.user_duration, Some(1800));

        // Test author not in list
        let session = LeaderboardSessionData::from_entries(entries, 999);
        assert_eq!(session.user_rank, None);
        assert_eq!(session.user_duration, None);
    }

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
