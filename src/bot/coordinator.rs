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

const MAX_NAV_HISTORY: usize = 10;

pub struct Coordinator<'a, S = ()> {
    ctx: Context<'a>,
    state: S,
    nav_queue: Mutex<VecDeque<NavigationResult>>,
}

impl<'a> Coordinator<'a, ()> {
    pub fn new(ctx: Context<'a>) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            state: (),
            nav_queue: Mutex::new(VecDeque::new()),
        })
    }
}

impl<'a, S: Send + Sync + 'static> Coordinator<'a, S> {
    pub fn with_state(ctx: Context<'a>, state: S) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            state,
            nav_queue: Mutex::new(VecDeque::new()),
        })
    }

    pub fn context(&self) -> &Context<'a> {
        &self.ctx
    }

    pub fn state(&self) -> &S {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    pub fn navigate(&self, next: NavigationResult) {
        let mut queue = self.nav_queue.lock().unwrap();
        if queue.len() >= MAX_NAV_HISTORY {
            queue.pop_front();
        }
        queue.push_back(next);
    }

    pub async fn run(self: Arc<Self>, initial: NavigationResult) -> Result<(), Error> {
        self.navigate(initial);
        let ctx = self.context();
        while let Some(mut controller) = self.next_controller(ctx) {
            controller.run(self.clone()).await?;
        }
        Ok(())
    }

    fn pop_next(&self) -> Option<NavigationResult> {
        self.nav_queue.lock().unwrap().pop_back()
    }

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
