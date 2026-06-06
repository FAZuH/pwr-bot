//! Bot command organization using the Cog pattern.
//!
//! This module provides a way to group and aggregate Discord commands using the
//! [`Cog`] trait. This structure allows for modular command definitions across
//! different files and domains.

pub mod about;
pub mod dump_db;
pub mod feed;
pub mod gui_test;
pub mod prelude;
pub mod register;
pub mod register_owner;
pub mod settings;
pub mod unregister;
pub mod voice;
pub mod welcome;

/// Error type used across bot commands.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Context type passed to command handlers.
///
/// Wraps the Poise context with application-specific [`Data`].
pub type Context<'a> = poise::Context<'a, Data, Error>;

use std::collections::VecDeque;
use std::sync::Arc;

use poise::Command;
use poise::ReplyHandle;

use crate::bot::Data;
use crate::bot::command::about::AboutHandler;
use crate::bot::command::feed::list::FeedListHandler;
use crate::bot::command::feed::settings::FeedSettingsHandler;
use crate::bot::command::feed::subscribe::FeedSubscribeHandler;
use crate::bot::command::feed::unsubscribe::FeedUnsubscribeHandler;
use crate::bot::command::settings::SettingsMainHandler;
use crate::bot::command::voice::leaderboard::VoiceLeaderboardHandler;
use crate::bot::command::voice::settings::VoiceSettingsHandler;
use crate::bot::command::voice::stats::VoiceStatsHandler;
use crate::bot::command::welcome::WelcomeSettingsHandler;
use crate::bot::navigation::Navigation;

/// Trait for command modules (Cogs) that provide a set of Discord commands.
///
/// A "Cog" is a collection of related commands (e.g., all feed-related commands).
pub trait Cog {
    /// Returns the list of commands provided by this cog.
    fn commands(&self) -> Vec<Command<Data, Error>>;
}

/// Aggregator for all command cogs in the application.
///
/// Implements [`Cog`] by collecting commands from all sub-modules.
pub struct Cogs;

impl Cog for Cogs {
    /// Collects and returns all registered commands for the bot.
    fn commands(&self) -> Vec<Command<Data, Error>> {
        vec![
            about::about(),
            dump_db::dump_db(),
            feed::feed(),
            gui_test::gui_test(),
            register::register(),
            register_owner::register_owner(),
            settings::settings(),
            unregister::unregister(),
            voice::voice(),
            welcome::welcome(),
        ]
    }
}

/// Maximum number of navigation steps to keep in history.
pub const MAX_NAV_HISTORY: usize = 10;

type SyncReplyHandle<'a> = tokio::sync::Mutex<Option<ReplyHandle<'a>>>;
type NavHistory = tokio::sync::Mutex<VecDeque<Navigation>>;

/// Orchestrator for command navigation.
///
/// The `Coordinator` owns the Poise command context
/// It maintains a history of [`Navigation`]s to support "Back" navigation.
pub struct Router<'a> {
    /// Poise command context.
    ctx: Context<'a>,
    /// Stack of navigation steps for history tracking.
    nav_queue: NavHistory,
    /// Shared handle to the active message.
    reply_handle: SyncReplyHandle<'a>,
}

impl<'a> Router<'a> {
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
    /// The loop continues as long as handlers return [`Navigation`]s,
    /// stopping when [`Navigation::Exit`] is reached or the history stack is empty.
    pub async fn run(self: Arc<Self>, initial: Navigation) -> Result<(), Error> {
        self.navigate(initial).await;
        while let Some(mut handler) = self.next_handler().await {
            handler.run(self.clone()).await?;
        }
        Ok(())
    }

    /// Pops the last navigation result from history.
    async fn pop_next(&self) -> Option<Navigation> {
        self.nav_queue.lock().await.pop_back()
    }

    /// Instantiates the next handler based on the current navigation state.
    async fn next_handler(&self) -> Option<Box<dyn CommandHandler + 'a>> {
        use Navigation::*;
        let ctx = self.ctx;

        loop {
            let nav = self.pop_next().await?;
            let res: Box<dyn CommandHandler> = match nav {
                SettingsMain => Box::new(SettingsMainHandler::new(ctx)),
                SettingsFeeds => Box::new(FeedSettingsHandler::new(ctx)),
                SettingsVoice => Box::new(VoiceSettingsHandler::new(ctx)),
                SettingsWelcome => Box::new(WelcomeSettingsHandler::new(ctx)),
                SettingsAbout => Box::new(AboutHandler::new(ctx)),
                FeedSubscriptions { send_into } => Box::new(FeedListHandler::new(ctx, send_into?)),
                FeedSubscribe { links, send_into } => {
                    Box::new(FeedSubscribeHandler::new(ctx, links, send_into))
                }
                FeedUnsubscribe { links, send_into } => {
                    Box::new(FeedUnsubscribeHandler::new(ctx, links, send_into))
                }
                FeedList(send_into) => Box::new(FeedListHandler::new(ctx, send_into?)),
                VoiceLeaderboard { time_range } => {
                    Box::new(VoiceLeaderboardHandler::new(ctx, time_range))
                }
                VoiceStats {
                    time_range,
                    target_user,
                    stat_type,
                } => Box::new(VoiceStatsHandler::new(
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

#[async_trait::async_trait]
pub trait CommandHandler: Send + Sync {
    /// Executes the handler logic.
    ///
    /// The `coordinator` provides access to shared state and navigation.
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error>;
}
