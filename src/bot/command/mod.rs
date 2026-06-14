//! Bot command organization using the Cog pattern.
//!
//! This module provides a way to group and aggregate Discord commands using the
//! [`Cog`] trait. This structure allows for modular command definitions across
//! different files and domains.

pub mod about;
pub mod dump_db;
pub mod gui_test;
pub mod prelude;
pub mod register;
pub mod register_owner;
pub mod settings;
pub mod unregister;

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
use crate::bot::command::settings::SettingsMainHandler;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::navigation::Navigation;
use crate::bot::plugin::invocation::dispatch_plugin_command;

/// Trait for command modules (Cogs) that provide a set of Discord commands.
///
/// A "Cog" is a collection of related commands registered by the core.
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
            gui_test::gui_test(),
            register::register(),
            register_owner::register_owner(),
            settings::settings(),
            unregister::unregister(),
        ]
    }
}

/// Maximum number of navigation steps to keep in history.
pub const MAX_NAV_HISTORY: usize = 10;

type SyncReplyHandle<'a> = tokio::sync::Mutex<Option<ReplyHandle<'a>>>;
type NavHistory = tokio::sync::Mutex<VecDeque<Navigation>>;

/// Orchestrator for command navigation.
///
/// The `Router` owns the Poise command context and maintains a history of
/// [`Navigation`]s to support "Back" navigation. It also carries a
/// [`PoiseHostCtx`] for abstracted access to the Discord interaction.
pub struct Router<'a> {
    /// Poise command context.
    ctx: Context<'a>,
    /// Stack of navigation steps for history tracking.
    nav_queue: NavHistory,
    /// Shared handle to the active message.
    reply_handle: SyncReplyHandle<'a>,
    /// Abstracted bot context for handler boilerplate.
    host_ctx: Arc<PoiseHostCtx>,
}

impl<'a> Router<'a> {
    /// Creates a new Router from a poise context.
    pub fn new(ctx: Context<'a>) -> Arc<Self> {
        let host_ctx = PoiseHostCtx::new(ctx);
        Arc::new(Self {
            ctx,
            nav_queue: tokio::sync::Mutex::new(VecDeque::new()),
            reply_handle: tokio::sync::Mutex::new(None),
            host_ctx,
        })
    }

    /// Returns the Poise command context.
    pub fn context(&self) -> &Context<'a> {
        &self.ctx
    }

    /// Returns the abstracted bot context.
    pub fn host_ctx(&self) -> &Arc<PoiseHostCtx> {
        &self.host_ctx
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

    /// Stores the reply handle for the current message.
    ///
    /// Used by handlers to get access to the sent message for later edits.
    pub async fn set_reply_handle(&self, new_reply: ReplyHandle<'a>) {
        *self.reply_handle.lock().await = Some(new_reply)
    }

    /// Returns a lock guard to the current reply handle.
    pub async fn reply_handle(&self) -> tokio::sync::MutexGuard<'_, Option<ReplyHandle<'a>>> {
        self.reply_handle.lock().await
    }

    /// Starts the navigation loop with an initial destination.
    ///
    /// The loop continues as long as handlers return [`Navigation`]s,
    /// stopping when the history stack is empty.
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

        let nav = self.pop_next().await?;
        let hc = self.host_ctx.clone();
        let res: Box<dyn CommandHandler> = match nav {
            SettingsMain => Box::new(SettingsMainHandler::new(hc)),
            SettingsPlugin { plugin_id } => Box::new(PluginSettingsHandler::new(hc, plugin_id)),
            SettingsAbout => Box::new(AboutHandler::new(hc)),
        };
        Some(res)
    }
}

#[async_trait::async_trait]
pub trait CommandHandler: Send + Sync {
    /// Executes the handler logic.
    ///
    /// The `coordinator` provides access to shared state and navigation.
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error>;
}

/// Handler that dispatches a plugin's settings panel via [`dispatch_plugin_command`].
///
/// The plugin's `invoke()` receives `"{name} settings"` as the command string.
pub struct PluginSettingsHandler {
    plugin_id: String,
    host_ctx: Arc<PoiseHostCtx>,
}

impl PluginSettingsHandler {
    /// Creates a new handler for the given plugin's settings panel.
    pub fn new(host_ctx: Arc<PoiseHostCtx>, plugin_id: String) -> Self {
        Self {
            plugin_id,
            host_ctx,
        }
    }
}

#[async_trait::async_trait]
impl CommandHandler for PluginSettingsHandler {
    async fn run(&mut self, coordinator: std::sync::Arc<Router<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        let registry = &ctx.data().plugin_registry;
        let cmd = format!("{} settings", self.plugin_id);
        dispatch_plugin_command(registry, &self.host_ctx, &cmd, serde_json::Value::Null).await
    }
}
