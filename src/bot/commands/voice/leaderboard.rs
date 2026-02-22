//! Voice leaderboard subcommand.

use std::ops::Deref;
use std::time::Duration;
use std::time::Instant;

use log::trace;
use poise::ChoiceParameter;
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
use crate::bot::views::ActionRegistry;
use crate::bot::views::ResponseKind;
use crate::bot::views::Trigger;
use crate::bot::views::ViewCommand;
use crate::bot::views::ViewContext;
use crate::bot::views::ViewEngine;
use crate::bot::views::ViewHandler;
use crate::bot::views::ViewRender;
use crate::bot::views::pagination::PaginationAction;
use crate::bot::views::pagination::PaginationView;
use crate::entity::VoiceLeaderboardEntry;
use crate::entity::VoiceLeaderboardOptBuilder;
use crate::error::AppError;

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
        let guild_id = ctx.guild_id().map(|id| id.get()).unwrap_or(0);
        let author_id = ctx.author().id.get();

        let mut view = VoiceLeaderboardHandler {
            pagination: PaginationView::new(session_data.len() as u32, LEADERBOARD_PER_PAGE),
            leaderboard_data: session_data,
            time_range: self.time_range,
            page_builder: LeaderboardPageBuilder::new(&ctx),
            current_page_bytes: None,
            is_partner_mode: false,
            target_user: None,
            service: ctx.data().service.voice_tracking.clone(),
            guild_id,
            author_id,
            http: ctx.serenity_context().http.clone(),
        };

        if view.leaderboard_data.is_empty() {
            let mut engine = ViewEngine::new(&ctx, view, Duration::from_millis(1));
            engine
                .run(|_| Box::pin(async { ViewCommand::Exit }))
                .await?;
            return Ok(NavigationResult::Exit);
        }

        // Generate and send initial page
        let page_result = view.generate_current_page().await?;
        view.set_current_page_bytes(Some(page_result.image_bytes));

        let mut engine = ViewEngine::new(&ctx, view, Duration::from_secs(120));

        trace!(
            "controller_initial_response {} ms",
            controller_start.elapsed().as_millis()
        );

        engine
            .run(|_action| Box::pin(async move { ViewCommand::Render }))
            .await?;

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

pub struct VoiceLeaderboardHandler<'a> {
    pub leaderboard_data: LeaderboardSessionData,
    pub time_range: VoiceLeaderboardTimeRange,
    pub pagination: PaginationView,
    pub page_builder: LeaderboardPageBuilder<'a>,
    pub current_page_bytes: Option<Vec<u8>>,
    pub is_partner_mode: bool,
    pub target_user: Option<serenity::all::User>,
    pub service: std::sync::Arc<crate::service::voice_tracking_service::VoiceTrackingService>,
    pub guild_id: u64,
    pub author_id: u64,
    pub http: std::sync::Arc<serenity::all::Http>,
}

impl<'a> VoiceLeaderboardHandler<'a> {
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

    /// Sets the current page image bytes for attachment on edit.
    pub fn set_current_page_bytes(&mut self, bytes: Option<Vec<u8>>) {
        self.current_page_bytes = bytes;
    }

    pub async fn refetch_data(&mut self) -> Result<(), Error> {
        let (since, until) = self.time_range.to_range();

        let voice_lb_opts = VoiceLeaderboardOptBuilder::default()
            .guild_id(self.guild_id)
            .limit(Some(u32::MAX))
            .since(Some(since))
            .until(Some(until))
            .build()
            .map_err(AppError::from)?;

        let new_entries = if self.is_partner_mode {
            let target_id = self
                .target_user
                .as_ref()
                .map(|u| u.id.get())
                .unwrap_or(self.author_id);
            self.service
                .get_partner_leaderboard(&voice_lb_opts, target_id)
                .await
                .map_err(Error::from)?
        } else {
            self.service
                .get_leaderboard_withopt(&voice_lb_opts)
                .await
                .map_err(Error::from)?
        };

        self.leaderboard_data = LeaderboardSessionData::from_entries(new_entries, self.author_id);

        // Update pagination
        self.pagination =
            PaginationView::new(self.leaderboard_data.len() as u32, LEADERBOARD_PER_PAGE);

        if !self.leaderboard_data.is_empty() {
            let page_result = self.generate_current_page().await?;
            self.set_current_page_bytes(Some(page_result.image_bytes));
        } else {
            self.set_current_page_bytes(None);
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl<'a> ViewHandler<VoiceLeaderboardAction> for VoiceLeaderboardHandler<'a> {
    async fn handle(
        &mut self,
        action: VoiceLeaderboardAction,
        _trigger: Trigger<'_>,
        _ctx: &ViewContext<'_, VoiceLeaderboardAction>,
    ) -> Result<ViewCommand, Error> {
        use VoiceLeaderboardAction::*;

        let mut changed_page = false;
        let mut fetch_new = false;

        if let Base(pagination_action) = action {
            let _cmd = self
                .pagination
                .handle(
                    pagination_action,
                    _trigger,
                    &_ctx.map(VoiceLeaderboardAction::Base),
                )
                .await?;

            changed_page = true;
        } else {
            match action {
                TimeRange => {
                    if let Trigger::Component(interaction) = &_trigger
                        && let ComponentInteractionDataKind::StringSelect { values } =
                            &interaction.data.kind
                        && let Some(time_range) = values
                            .first()
                            .and_then(|v| VoiceLeaderboardTimeRange::from_display_name(v))
                        && self.time_range != time_range
                    {
                        self.time_range = time_range;
                        fetch_new = true;
                    }
                }
                ToggleMode => {
                    self.is_partner_mode = !self.is_partner_mode;
                    fetch_new = true;
                }
                SelectUser => {
                    if let Trigger::Component(interaction) = &_trigger
                        && let ComponentInteractionDataKind::UserSelect { values } =
                            &interaction.data.kind
                        && let Some(user_id) = values.first()
                        && let Ok(user) = user_id.to_user(&self.http).await
                    {
                        self.target_user = Some(user);
                        fetch_new = true;
                    }
                }
                _ => {}
            }
        }

        if fetch_new {
            self.refetch_data().await?;
        } else if changed_page && !self.leaderboard_data.is_empty() {
            let page_result = self.generate_current_page().await?;
            self.set_current_page_bytes(Some(page_result.image_bytes));
        }

        Ok(ViewCommand::Render)
    }
}

impl<'a> ViewRender<VoiceLeaderboardAction> for VoiceLeaderboardHandler<'a> {
    fn render(&self, registry: &mut ActionRegistry<VoiceLeaderboardAction>) -> ResponseKind<'_> {
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
        let toggle_button = serenity::all::CreateButton::new(registry.register(ToggleMode))
            .label(toggle_label)
            .style(serenity::all::ButtonStyle::Primary);

        container.push(CreateContainerComponent::ActionRow(
            CreateActionRow::Buttons(vec![toggle_button].into()),
        ));

        let time_range_menu = serenity::all::CreateSelectMenu::new(
            registry.register(TimeRange),
            CreateSelectMenuKind::String {
                options: vec![
                    Past24Hours.into(),
                    Past72Hours.into(),
                    Past7Days.into(),
                    Past14Days.into(),
                    ThisMonth.into(),
                    ThisYear.into(),
                    AllTime.into(),
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

        if self.is_partner_mode {
            let default_users = self
                .target_user
                .clone()
                .map(|u| std::borrow::Cow::Owned(vec![u.id]));
            let user_select = serenity::all::CreateSelectMenu::new(
                registry.register(SelectUser),
                CreateSelectMenuKind::User { default_users },
            )
            .placeholder("Select a user to view their voice partners");
            components.push(CreateComponent::ActionRow(CreateActionRow::SelectMenu(
                user_select,
            )));
        }

        self.pagination
            .attach_if_multipage(registry, &mut components, |action| {
                VoiceLeaderboardAction::Base(action)
            });

        components.into()
    }

    fn create_reply(
        &self,
        registry: &mut ActionRegistry<VoiceLeaderboardAction>,
    ) -> poise::CreateReply<'_> {
        let response = self.render(registry);
        let mut reply: poise::CreateReply<'_> = response.into();

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
        let (since, until) = VoiceLeaderboardTimeRange::Past24Hours.to_range();
        assert!(since <= until);

        let (since, until) = VoiceLeaderboardTimeRange::AllTime.to_range();
        assert_eq!(since, chrono::DateTime::UNIX_EPOCH);
        assert!(since <= until);
    }

    #[test]
    fn test_voice_leaderboard_time_range_into_datetime() {
        let now = chrono::Utc::now();
        let past_24h_start: chrono::DateTime<chrono::Utc> =
            VoiceLeaderboardTimeRange::Past24Hours.into();
        assert!(past_24h_start <= now);

        let all_time_start: chrono::DateTime<chrono::Utc> =
            VoiceLeaderboardTimeRange::AllTime.into();
        assert_eq!(all_time_start, chrono::DateTime::UNIX_EPOCH);
    }

    #[test]
    fn test_voice_leaderboard_time_range_equality() {
        let range1 = VoiceLeaderboardTimeRange::Past24Hours;
        let range2 = VoiceLeaderboardTimeRange::Past24Hours;
        let range3 = VoiceLeaderboardTimeRange::ThisMonth;

        assert_eq!(range1, range2);
        assert_ne!(range1, range3);
    }
}
