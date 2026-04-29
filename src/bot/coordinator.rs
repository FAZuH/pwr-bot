//! Navigation coordinator for the MVC-C pattern.
//!
//! The `Coordinator` drives the interaction flow of a command by managing
//! a stack of [`Controller`]s and processing [`Navigation`]s.

use std::collections::VecDeque;
use std::sync::Arc;

use poise::ReplyHandle;

use crate::bot::Error;
use crate::bot::command::Context;
use crate::bot::command::about::AboutController;
use crate::bot::command::feed::list::FeedListController;
use crate::bot::command::feed::settings::FeedSettingsController;
use crate::bot::command::feed::subscribe::FeedSubscribeController;
use crate::bot::command::feed::unsubscribe::FeedUnsubscribeController;
use crate::bot::command::settings::SettingsMainController;
use crate::bot::command::voice::leaderboard::VoiceLeaderboardController;
use crate::bot::command::voice::settings::VoiceSettingsController;
use crate::bot::command::voice::stats::VoiceStatsController;
use crate::bot::command::welcome::WelcomeSettingsController;
use crate::bot::controller::Controller;
use crate::bot::navigation::Navigation;

/// Maximum number of navigation steps to keep in history.
pub const MAX_NAV_HISTORY: usize = 10;

type SyncReplyHandle<'a> = tokio::sync::Mutex<Option<ReplyHandle<'a>>>;
type NavHistory = tokio::sync::Mutex<VecDeque<Navigation>>;

/// Orchestrator for controller navigation and shared state.
///
/// The `Coordinator` owns the Poise command context
/// It maintains a history of [`Navigation`]s to support "Back" navigation.
pub struct Coordinator<'a> {
    /// Poise command context.
    ctx: Context<'a>,
    /// Stack of navigation steps for history tracking.
    nav_queue: NavHistory,
    /// Shared handle to the active message.
    reply_handle: SyncReplyHandle<'a>,
}

impl<'a> Coordinator<'a> {
    /// Creates a new coordinator.
    pub fn new(ctx: Context<'a>) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            nav_queue: tokio::sync::Mutex::new(VecDeque::new()),
            reply_handle: tokio::sync::Mutex::new(None),
        })
    }

    /// Returns the Poise context.
    pub fn context(&self) -> &Context<'a> {
        &self.ctx
    }

    /// Pushes a new navigation target onto the stack.
    ///
    /// If history exceeds [`MAX_NAV_HISTORY`], the oldest step is removed.
    pub async fn navigate(&self, next: Navigation) {
        let mut queue = self.nav_queue.lock().await;
        if queue.len() >= MAX_NAV_HISTORY {
            queue.pop_front();
        }
        queue.push_back(next);
    }

    /// Returns the most recent navigation target without removing it.
    pub async fn peek_navigation(&self) -> Option<Navigation> {
        self.nav_queue.lock().await.back().cloned()
    }

    pub async fn set_reply_handle(&self, new_reply: ReplyHandle<'a>) {
        *self.reply_handle.lock().await = Some(new_reply)
    }

    pub async fn reply_handle(&self) -> tokio::sync::MutexGuard<'_, Option<ReplyHandle<'a>>> {
        self.reply_handle.lock().await
    }

    /// Starts the navigation loop with an initial destination.
    ///
    /// The loop continues as long as controllers return [`Navigation`]s,
    /// stopping when [`Navigation::Exit`] is reached or the history stack is empty.
    pub async fn run(self: Arc<Self>, initial: Navigation) -> Result<(), Error> {
        self.navigate(initial).await;
        while let Some(mut controller) = self.next_controller().await {
            controller.run(self.clone()).await?;
        }
        Ok(())
    }

    /// Pops the last navigation result from history.
    async fn pop_next(&self) -> Option<Navigation> {
        self.nav_queue.lock().await.pop_back()
    }

    /// Instantiates the next controller based on the current navigation state.
    async fn next_controller(&self) -> Option<Box<dyn Controller + 'a>> {
        use Navigation::*;
        let ctx = self.ctx;

        loop {
            let nav = self.pop_next().await?;
            let res: Box<dyn Controller> = match nav {
                SettingsMain => Box::new(SettingsMainController::new(ctx)),
                SettingsFeeds => Box::new(FeedSettingsController::new(ctx)),
                SettingsVoice => Box::new(VoiceSettingsController::new(ctx)),
                SettingsWelcome => Box::new(WelcomeSettingsController::new(ctx)),
                SettingsAbout => Box::new(AboutController::new(ctx)),
                FeedSubscriptions { send_into } => {
                    Box::new(FeedListController::new(ctx, send_into?))
                }
                FeedSubscribe { links, send_into } => {
                    Box::new(FeedSubscribeController::new(ctx, links, send_into))
                }
                FeedUnsubscribe { links, send_into } => {
                    Box::new(FeedUnsubscribeController::new(ctx, links, send_into))
                }
                FeedList(send_into) => Box::new(FeedListController::new(ctx, send_into?)),
                VoiceLeaderboard { time_range } => {
                    Box::new(VoiceLeaderboardController::new(ctx, time_range))
                }
                VoiceStats {
                    time_range,
                    target_user,
                    stat_type,
                } => Box::new(VoiceStatsController::new(
                    ctx,
                    time_range,
                    *target_user,
                    stat_type,
                )),
                Back => continue,
                Exit => return None,
            };
            return Some(res);
        }
    }
}
