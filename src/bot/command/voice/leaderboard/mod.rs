//! Voice leaderboard subcommand.
use std::ops::Deref;
use std::time::Duration;
use std::time::Instant;

use log::trace;

use crate::bot::command::prelude::*;
use crate::bot::command::voice::TimeRange;
use crate::bot::command::voice::VoiceLeaderboardTimeRange;
use crate::bot::command::voice::leaderboard::image_builder::LeaderboardImageBuilder;
use crate::bot::view::pagination::PaginationAction;
use crate::bot::view::pagination::PaginationView;
use crate::entity::VoiceLeaderboardEntry;
use crate::entity::VoiceLeaderboardOptBuilder;
use crate::service::traits::VoiceTracker;
use crate::update::Update;
use crate::update::voice_leaderboard::VoiceLeaderboardCmd;
use crate::update::voice_leaderboard::VoiceLeaderboardModel;
use crate::update::voice_leaderboard::VoiceLeaderboardMsg;
use crate::update::voice_leaderboard::VoiceLeaderboardUpdate;

pub mod image_builder;
pub mod image_generator;

/// Filename for the voice leaderboard image attachment.
pub const IMAGE_FILENAME: &str = "voice_leaderboard.jpg";

/// Number of leaderboard entries per page.
pub const LEADERBOARD_PER_PAGE: u32 = 10;

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
    Coordinator::new(ctx)
        .run(NavigationResult::VoiceLeaderboard {
            time_range: time_range.unwrap_or(VoiceLeaderboardTimeRange::ThisMonth),
        })
        .await?;
    Ok(())
}

/// Data for a leaderboard session.
#[derive(Debug, Clone, Default)]
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
    ctx: Context<'a>,
    pub time_range: VoiceLeaderboardTimeRange,
}

impl<'a> VoiceLeaderboardController<'a> {
    /// Creates a new leaderboard controller.
    pub fn new(ctx: Context<'a>, time_range: VoiceLeaderboardTimeRange) -> Self {
        Self { ctx, time_range }
    }

    /// Fetches leaderboard entries for the current time range.
    async fn fetch_entries(
        ctx: &Context<'_>,
        time_range: VoiceLeaderboardTimeRange,
        is_partner_mode: bool,
        target_user: Option<poise::serenity_prelude::UserId>,
    ) -> Result<Vec<VoiceLeaderboardEntry>, Error> {
        let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
        let (since, until) = time_range.to_range();

        let voice_lb_opts = VoiceLeaderboardOptBuilder::default()
            .guild_id(guild_id)
            .limit(Some(u32::MAX))
            .since(Some(since))
            .until(Some(until))
            .build()
            .map_err(AppError::from)?;

        let entries = if is_partner_mode {
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

        Ok(entries)
    }
}

#[async_trait::async_trait]
impl Controller for VoiceLeaderboardController<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Coordinator<'_>>) -> Result<(), Error> {
        let controller_start = Instant::now();

        let ctx = *coordinator.context();
        ctx.defer().await?;

        // Fetch initial entries
        let entries = Self::fetch_entries(&ctx, self.time_range, false, None).await?;
        let guild_id = ctx.guild_id().map(|id| id.get()).unwrap_or(0);
        let author_id = ctx.author().id.get();
        let model = VoiceLeaderboardModel::from_entries(entries, author_id, LEADERBOARD_PER_PAGE);

        let view = VoiceLeaderboardHandler::new(model, &ctx, guild_id, author_id);

        let mut engine = ViewEngine::new(ctx, view, Duration::from_mins(2), coordinator.clone());
        engine.run().await?;

        trace!(
            "controller_total {} ms",
            controller_start.elapsed().as_millis()
        );
        Ok(())
    }
}

pub struct VoiceLeaderboardHandler<'a> {
    pub model: VoiceLeaderboardModel,
    pub img_builder: LeaderboardImageBuilder<'a>,
    pub lb_img: Option<Vec<u8>>,
    pub target_user: Option<poise::serenity_prelude::User>,
    pub service: std::sync::Arc<dyn VoiceTracker>,
    pub guild_id: u64,
    pub author_id: u64,
    pub http: std::sync::Arc<poise::serenity_prelude::Http>,
    pub pagination: bool,
}

impl<'a> VoiceLeaderboardHandler<'a> {
    pub fn new(
        model: VoiceLeaderboardModel,
        ctx: &'a Context<'a>,
        guild_id: u64,
        author_id: u64,
    ) -> Self {
        Self {
            pagination: model.is_empty(),
            model,
            lb_img: None,
            target_user: None,
            service: ctx.data().service.voice_tracking.clone(),
            guild_id,
            author_id,
            http: ctx.serenity_context().http.clone(),
            img_builder: LeaderboardImageBuilder::new(ctx),
        }
    }

    /// Generates the page image for the current page.
    async fn generate_img(&mut self) -> Result<(), Error> {
        if !self.model.is_empty() {
            let entries = self.model.current_page_entries();
            let rank_offset = self.model.current_page_rank_offset();
            let res = self.img_builder.build(entries, rank_offset).await;
            if let Ok(img) = res {
                self.lb_img = Some(img.image_bytes);
            }
        }
        Ok(())
    }

    async fn refetch_data(&mut self) -> Result<(), Error> {
        let (since, until) = self.model.time_range.to_range();

        let voice_lb_opts = VoiceLeaderboardOptBuilder::default()
            .guild_id(self.guild_id)
            .limit(Some(u32::MAX))
            .since(Some(since))
            .until(Some(until))
            .build()
            .map_err(AppError::from)?;

        let new_entries = if self.model.is_partner_mode {
            let target_id = self.model.target_user_id.unwrap_or(self.author_id);
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

        VoiceLeaderboardUpdate::update(
            VoiceLeaderboardMsg::SetEntries(new_entries),
            &mut self.model,
        );

        if !self.model.is_empty() {
            self.generate_img().await?;
        } else {
            self.lb_img = None;
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl ViewHandler for VoiceLeaderboardHandler<'_> {
    type Action = VoiceLeaderboardAction;
    async fn handle(
        &mut self,
        ctx: ViewContext<'_, VoiceLeaderboardAction>,
    ) -> Result<ViewCommand, Error> {
        use VoiceLeaderboardAction::*;

        let mut changed_page = false;
        let mut fetch_new = false;

        log::debug!("{:?}", ctx.event);
        match ctx.action() {
            Base(inner) => {
                let cmd = VoiceLeaderboardUpdate::update(
                    VoiceLeaderboardMsg::Pagination(*inner),
                    &mut self.model,
                );
                assert!(matches!(cmd, VoiceLeaderboardCmd::None));
                changed_page = true;
            }
            TimeRange => {
                if let Some(time_range) = ctx
                    .string_select_values()
                    .and_then(|v| v.first().cloned())
                    .and_then(|v| VoiceLeaderboardTimeRange::from_display_name(&v))
                {
                    let cmd = VoiceLeaderboardUpdate::update(
                        VoiceLeaderboardMsg::ChangeTimeRange(time_range),
                        &mut self.model,
                    );
                    fetch_new = matches!(cmd, VoiceLeaderboardCmd::RefetchData);
                }
            }
            ToggleMode => {
                let cmd = VoiceLeaderboardUpdate::update(
                    VoiceLeaderboardMsg::ToggleMode,
                    &mut self.model,
                );
                fetch_new = matches!(cmd, VoiceLeaderboardCmd::RefetchData);
            }
            SelectUser => {
                if let Some(user_id) = ctx.user_select_values().and_then(|v| v.first().copied())
                    && let Ok(user) = user_id.to_user(&self.http).await
                {
                    self.target_user = Some(user.clone());
                    let cmd = VoiceLeaderboardUpdate::update(
                        VoiceLeaderboardMsg::SetTargetUser(Some(user.id.get())),
                        &mut self.model,
                    );
                    fetch_new = matches!(cmd, VoiceLeaderboardCmd::RefetchData);
                }
            }
        }

        if fetch_new {
            self.refetch_data().await?;
        }
        if changed_page && !self.model.is_empty() {
            self.generate_img().await?
        }

        Ok(ViewCommand::Render)
    }

    async fn on_timeout(&mut self) -> Result<ViewCommand, Error> {
        self.pagination = false;
        if self.model.pages() > 1 {
            Ok(ViewCommand::RenderOnce)
        } else {
            Ok(ViewCommand::Exit)
        }
    }
}

impl ViewRender for VoiceLeaderboardHandler<'_> {
    type Action = VoiceLeaderboardAction;
    fn render(&self, registry: &mut ActionRegistry<VoiceLeaderboardAction>) -> ResponseKind<'_> {
        use VoiceLeaderboardAction::*;
        use VoiceLeaderboardTimeRange::*;

        let mut container = vec![CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(if self.model.is_partner_mode {
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

        if let Some(rank) = self.model.user_rank {
            let duration_text = self
                .model
                .user_duration
                .map(format_duration)
                .unwrap_or_else(|| "unknown".to_string());

            container.push(CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new(format!(
                    "\nYou are ranked **#{}** on this server with **{}** of voice activity.",
                    rank, duration_text
                )),
            ));
        } else if !self.model.target_is_author() {
            container.push(CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new("\nYou are not on the leaderboard for this time range."),
            ));
        }

        let (since, until) = self.model.time_range.to_range();
        container.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(format!(
                "\n-# Time Range: **{}** — <t:{}:f> to <t:{}:R>",
                self.model.time_range.name(),
                since.timestamp(),
                until.timestamp(),
            )),
        ));

        container.push(CreateContainerComponent::Separator(CreateSeparator::new(
            true,
        )));

        if self.model.is_empty() {
            container.push(CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new(
                    "No voice activity recorded yet at this time range.\n\nJoin a **voice channel** to start tracking!",
                ),
            ));
        } else {
            container.push(CreateContainerComponent::MediaGallery(
                CreateMediaGallery::new(vec![CreateMediaGalleryItem::new(
                    CreateUnfurledMediaItem::new(format!("attachment://{}", IMAGE_FILENAME)),
                )]),
            ));
        }

        let toggle_label = if self.model.is_partner_mode {
            "Show Server Leaderboard"
        } else {
            "Show Voice Partners"
        };
        let toggle_button = registry
            .register(ToggleMode)
            .as_button()
            .label(toggle_label)
            .style(poise::serenity_prelude::ButtonStyle::Primary);

        container.push(CreateContainerComponent::ActionRow(
            CreateActionRow::Buttons(vec![toggle_button].into()),
        ));

        let time_range_menu = registry
            .register(TimeRange)
            .as_select(CreateSelectMenuKind::String {
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
            })
            .placeholder("Select time range");

        let action_row = CreateActionRow::SelectMenu(time_range_menu);

        let mut components = vec![
            CreateComponent::Container(CreateContainer::new(container)),
            CreateComponent::ActionRow(action_row),
        ];

        if self.model.is_partner_mode {
            let default_users = self
                .target_user
                .clone()
                .map(|u| std::borrow::Cow::Owned(vec![u.id]));
            let user_select = registry
                .register(SelectUser)
                .as_select(CreateSelectMenuKind::User { default_users })
                .placeholder("Select an user to view their voice partners");
            components.push(CreateComponent::ActionRow(CreateActionRow::SelectMenu(
                user_select,
            )));
        }

        let mut pagination =
            PaginationView::new(self.model.entries.len() as u32, LEADERBOARD_PER_PAGE);
        pagination.state.current_page = self.model.current_page;
        pagination.disabled = self.pagination;
        pagination.attach_if_multipage(registry, &mut components, |action| {
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

        if let Some(ref bytes) = self.lb_img {
            let attachment = CreateAttachment::bytes(bytes.clone(), IMAGE_FILENAME);
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
    use chrono::DateTime;
    use chrono::Datelike;
    use chrono::Utc;

    use super::*;
    use crate::bot::command::voice::leaderboard::image_builder::LeaderboardEntry;

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
        assert_eq!(since, DateTime::UNIX_EPOCH);
        assert!(since <= until);
    }

    #[test]
    fn test_voice_leaderboard_time_range_into_datetime() {
        let now = Utc::now();
        let past_24h_start: DateTime<Utc> = VoiceLeaderboardTimeRange::Past24Hours.into();
        assert!(past_24h_start <= now);

        let all_time_start: DateTime<Utc> = VoiceLeaderboardTimeRange::AllTime.into();
        assert_eq!(all_time_start, DateTime::UNIX_EPOCH);
    }

    #[test]
    fn test_voice_leaderboard_time_range_equality() {
        let range1 = VoiceLeaderboardTimeRange::Past24Hours;
        let range2 = VoiceLeaderboardTimeRange::Past24Hours;
        let range3 = VoiceLeaderboardTimeRange::ThisMonth;

        assert_eq!(range1, range2);
        assert_ne!(range1, range3);
    }

    #[test]
    fn test_format_duration_edge_cases() {
        // Test zero
        assert_eq!(format_duration(0), "0s");

        // Test boundaries
        assert_eq!(format_duration(59), "59s");
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(61), "1m");

        assert_eq!(format_duration(3599), "59m");
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3601), "1h");

        assert_eq!(format_duration(86399), "23h 59m");
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(86401), "1d");
    }

    #[test]
    fn test_format_duration_comprehensive() {
        // Seconds
        assert_eq!(format_duration(45), "45s");

        // Minutes
        assert_eq!(format_duration(300), "5m");
        assert_eq!(format_duration(1500), "25m");

        // Hours with and without minutes
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(7260), "2h 1m");
        assert_eq!(format_duration(9000), "2h 30m");

        // Days with and without hours
        assert_eq!(format_duration(172800), "2d");
        assert_eq!(format_duration(176400), "2d 1h");
        assert_eq!(format_duration(259200), "3d");

        // Large values
        assert_eq!(format_duration(604800), "7d"); // One week
        assert_eq!(format_duration(2592000), "30d"); // ~30 days
        assert_eq!(format_duration(31536000), "365d"); // ~1 year
    }

    #[test]
    fn test_voice_leaderboard_time_range_all_variants() {
        // Test all time range variants can be converted to datetime
        let ranges = vec![
            VoiceLeaderboardTimeRange::Today,
            VoiceLeaderboardTimeRange::Past7Days,
            VoiceLeaderboardTimeRange::Past14Days,
            VoiceLeaderboardTimeRange::ThisMonth,
            VoiceLeaderboardTimeRange::ThisYear,
            VoiceLeaderboardTimeRange::AllTime,
        ];

        let now = Utc::now();

        for range in ranges {
            let (since, until) = range.to_range();
            assert!(since <= until, "Time range {:?} has since > until", range);
            assert!(
                until >= now || range == VoiceLeaderboardTimeRange::AllTime,
                "Until should be around now for {:?}",
                range
            );

            // Verify round-trip through ChoiceParameter name
            let name = ChoiceParameter::name(&range);
            let recovered = VoiceLeaderboardTimeRange::from_name(name);
            assert!(
                recovered.is_some(),
                "Should be able to recover {:?} from name '{}'",
                range,
                name
            );
            assert_eq!(
                recovered.unwrap() as i32,
                range as i32,
                "Recovered range should match original"
            );
        }
    }

    #[test]
    fn test_time_range_date_boundaries() {
        let now = Utc::now();

        // Today should start at midnight today
        let today_start: DateTime<Utc> = VoiceLeaderboardTimeRange::Today.into();
        assert_eq!(
            today_start.time(),
            chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap()
        );
        assert_eq!(today_start.date_naive(), now.date_naive());

        // This month should start on the 1st
        let month_start: DateTime<Utc> = VoiceLeaderboardTimeRange::ThisMonth.into();
        assert_eq!(month_start.day(), 1);
        assert_eq!(
            month_start.time(),
            chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap()
        );

        // This year should start on Jan 1
        let year_start: DateTime<Utc> = VoiceLeaderboardTimeRange::ThisYear.into();
        assert_eq!(year_start.month(), 1);
        assert_eq!(year_start.day(), 1);

        // All time should be Unix epoch
        let all_time_start: DateTime<Utc> = VoiceLeaderboardTimeRange::AllTime.into();
        assert_eq!(all_time_start, DateTime::UNIX_EPOCH);
    }

    #[test]
    fn test_time_range_relative_durations() {
        let now = Utc::now();

        // This week should be within the last 7 days
        let (since, _) = VoiceLeaderboardTimeRange::Past7Days.to_range();
        let duration = now.signed_duration_since(since);
        assert!(
            duration.num_days() >= 0 && duration.num_days() <= 7,
            "This week should be within last 7 days, got {} days",
            duration.num_days()
        );

        // Past 2 weeks should be within the last 14 days
        let (since, _) = VoiceLeaderboardTimeRange::Past14Days.to_range();
        let duration = now.signed_duration_since(since);
        assert!(
            duration.num_days() >= 7 && duration.num_days() <= 14,
            "Past 2 weeks should be within last 14 days, got {} days",
            duration.num_days()
        );
    }

    #[test]
    fn test_leaderboard_entry_clone() {
        let entry = LeaderboardEntry {
            rank: 1,
            user_id: 123456789012345678,
            display_name: "Test User".to_string(),
            avatar_url: "https://cdn.discordapp.com/avatars/123/abc.png".to_string(),
            duration_seconds: 3600,
            avatar_image: None,
        };

        let cloned = entry.clone();
        assert_eq!(cloned.rank, entry.rank);
        assert_eq!(cloned.user_id, entry.user_id);
        assert_eq!(cloned.display_name, entry.display_name);
        assert_eq!(cloned.avatar_url, entry.avatar_url);
        assert_eq!(cloned.duration_seconds, entry.duration_seconds);
        assert!(cloned.avatar_image.is_none());
    }
}
