//! Navigation coordinator for the MVC-C pattern.
//!
//! The `Coordinator` drives the interaction flow of a command by managing
//! a stack of [`Controller`]s and processing [`NavigationResult`]s.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use crate::bot::Error;
use crate::bot::commands::Context;
use crate::bot::commands::about::AboutController;
use crate::bot::commands::feed::list::FeedListController;
use crate::bot::commands::feed::settings::FeedSettingsController;
use crate::bot::commands::feed::subscribe::FeedSubscribeController;
use crate::bot::commands::feed::unsubscribe::FeedUnsubscribeController;
use crate::bot::commands::settings::SettingsMainController;
use crate::bot::commands::voice::leaderboard::VoiceLeaderboardController;
use crate::bot::commands::voice::settings::VoiceSettingsController;
use crate::bot::commands::voice::stats::VoiceStatsController;
use crate::bot::commands::welcome::WelcomeSettingsController;
use crate::bot::controller::Controller;
use crate::bot::navigation::NavigationResult;

/// Maximum number of navigation steps to keep in history.
const MAX_NAV_HISTORY: usize = 10;

/// Orchestrator for controller navigation and shared state.
///
/// The `Coordinator` owns the Poise command context and an optional shared state `S`.
/// It maintains a history of [`NavigationResult`]s to support "Back" navigation.
pub struct Coordinator<'a, S = ()> {
    /// Poise command context.
    ctx: Context<'a>,
    /// Optional shared state accessible to all controllers.
    state: S,
    /// Stack of navigation steps for history tracking.
    nav_queue: Mutex<VecDeque<NavigationResult>>,
}

impl<'a> Coordinator<'a, ()> {
    /// Creates a new coordinator without shared state.
    pub fn new(ctx: Context<'a>) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            state: (),
            nav_queue: Mutex::new(VecDeque::new()),
        })
    }
}

impl<'a, S: Send + Sync + 'static> Coordinator<'a, S> {
    /// Creates a new coordinator with an initial shared state.
    pub fn with_state(ctx: Context<'a>, state: S) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            state,
            nav_queue: Mutex::new(VecDeque::new()),
        })
    }

    /// Returns the Poise context.
    pub fn context(&self) -> &Context<'a> {
        &self.ctx
    }

    /// Returns a reference to the shared state.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Returns a mutable reference to the shared state.
    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    /// Pushes a new navigation target onto the stack.
    ///
    /// If history exceeds [`MAX_NAV_HISTORY`], the oldest step is removed.
    pub fn navigate(&self, next: NavigationResult) {
        let mut queue = self.nav_queue.lock().unwrap();
        if queue.len() >= MAX_NAV_HISTORY {
            queue.pop_front();
        }
        queue.push_back(next);
    }

    /// Starts the navigation loop with an initial destination.
    ///
    /// The loop continues as long as controllers return [`NavigationResult`]s,
    /// stopping when [`NavigationResult::Exit`] is reached or the history stack is empty.
    pub async fn run(self: Arc<Self>, initial: NavigationResult) -> Result<(), Error> {
        self.navigate(initial);
        let ctx = self.context();
        while let Some(mut controller) = self.next_controller(ctx) {
            controller.run(self.clone()).await?;
        }
        Ok(())
    }

    /// Pops the last navigation result from history.
    fn pop_next(&self) -> Option<NavigationResult> {
        self.nav_queue.lock().unwrap().pop_back()
    }

    /// Instantiates the next controller based on the current navigation state.
    fn next_controller(&self, ctx: &'a Context<'a>) -> Option<Box<dyn Controller<S> + 'a>> {
        use NavigationResult::*;

        loop {
            let nav = self.pop_next()?;
            let res: Box<dyn Controller<S>> = match nav {
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
                Back => continue,
                Exit => return None,
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
            };
            return Some(res);
        }
    }
}
