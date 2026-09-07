//! Bot command organization using the Cog pattern.
//!
//! This module provides a way to group and aggregate Discord commands using the
//! [`Cog`] trait. This structure allows for modular command definitions across
//! different files and domains.

pub mod about;
pub mod dump_db;
pub mod feed;
pub mod plugins;
pub mod prelude;
pub mod register;
pub mod register_owner;
pub mod session_exit;
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

use log::warn;
use poise::Command;
use poise::ReplyHandle;

use crate::bot::Data;
use crate::bot::command::about::AboutHandler;
use crate::bot::command::feed::list::FeedListHandler;
use crate::bot::command::feed::subscribe::FeedSubscribeHandler;
use crate::bot::command::feed::unsubscribe::FeedUnsubscribeHandler;
use crate::bot::command::voice::leaderboard::VoiceLeaderboardHandler;
use crate::bot::command::voice::stats::VoiceStatsHandler;
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
            plugins::plugins(),
            register::register(),
            register_owner::register_owner(),
            unregister::unregister(),
            voice::voice(),
            welcome::welcome(),
        ]
    }
}

/// Maximum number of navigation targets kept queued, and of open frames kept
/// in a session's history: the oldest falls off either way.
pub const MAX_NAV_HISTORY: usize = 10;

type SyncReplyHandle<'a> = tokio::sync::Mutex<Option<ReplyHandle<'a>>>;
type NavQueue = tokio::sync::Mutex<VecDeque<Navigation>>;

/// Orchestrator for command navigation.
///
/// The `Router` owns the Poise command context and the message the session
/// renders into. It keeps the navigation targets handlers ask for; the
/// frames open on the message — the walk "Back" steps through — live in the
/// session loop it drives, see [`Router::run`].
pub struct Router<'a> {
    /// Poise command context.
    ctx: Context<'a>,
    /// Queue of navigation targets pending a run. The frames open on the
    /// message live in the session loop's history, not here.
    nav_queue: NavQueue,
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

    /// Queues a navigation target for the session loop to pop next.
    ///
    /// If the queue exceeds [`MAX_NAV_HISTORY`], the oldest target is
    /// removed.
    pub async fn navigate(&self, next: Navigation) {
        let mut queue = self.nav_queue.lock().await;
        if queue.len() >= MAX_NAV_HISTORY {
            queue.pop_front();
        }
        queue.push_back(next);
    }

    /// Returns the newest queued target without removing it.
    pub async fn peek_navigation(&self) -> Option<Navigation> {
        self.nav_queue.lock().await.back().cloned()
    }

    pub async fn set_reply_handle(&self, new_reply: ReplyHandle<'a>) {
        *self.reply_handle.lock().await = Some(new_reply)
    }

    pub async fn reply_handle(&self) -> tokio::sync::MutexGuard<'_, Option<ReplyHandle<'a>>> {
        self.reply_handle.lock().await
    }

    /// Resolves a popped navigation target into the handler that renders it.
    ///
    /// `None` ends the session: a feed list target that lost its delivery
    /// channel has nothing to render. The terminal targets never reach this
    /// map — [`pop_step`] resolves them into the hub handoff, the root
    /// dismissal, or the end of the session.
    fn handler_for(&self, target: Navigation) -> Option<Box<dyn CommandHandler + 'a>> {
        use Navigation::*;
        let ctx = self.ctx;
        match target {
            SettingsAbout => Some(Box::new(AboutHandler::new(ctx))),
            FeedSubscriptions { send_into } => {
                send_into.map(|send_into| Box::new(FeedListHandler::new(ctx, send_into)) as Box<_>)
            }
            FeedSubscribe { links, send_into } => {
                Some(Box::new(FeedSubscribeHandler::new(ctx, links, send_into)))
            }
            FeedUnsubscribe { links, send_into } => {
                Some(Box::new(FeedUnsubscribeHandler::new(ctx, links, send_into)))
            }
            FeedList(send_into) => {
                send_into.map(|send_into| Box::new(FeedListHandler::new(ctx, send_into)) as Box<_>)
            }
            VoiceLeaderboard { time_range } => {
                Some(Box::new(VoiceLeaderboardHandler::new(ctx, time_range)))
            }
            VoiceStats {
                time_range,
                target_user,
                stat_type,
            } => Some(Box::new(VoiceStatsHandler::new(
                ctx,
                time_range,
                *target_user,
                stat_type,
            ))),
            SettingsMain | Back | Exit => {
                unreachable!("pop_step resolves the terminal targets itself")
            }
        }
    }

    /// Ends the session on the live reply: one terminal step, fetched once.
    ///
    /// Both terminal steps share this shape over the reply handle — resolve
    /// the live reply, fetch its message once, act. The hub handoff morphs
    /// the message into the settings plugin's hub view, and a failed handoff
    /// falls back to the root dismissal; a root Back dismisses directly. The
    /// message is fetched once and passed to both halves, so the
    /// handoff-failure path cannot fail a refetch and silently skip the
    /// dismissal. Without a live reply, or when the fetch fails, the miss is
    /// logged and the session ends with the message as it is.
    async fn exit_through_reply(&self, root_back: bool) {
        let reply = self.reply_handle().await;
        let Some(reply) = reply.as_ref() else {
            warn!("session exit found no live reply; leaving the message as it is");
            return;
        };
        let Ok(message) = reply.message().await else {
            warn!("session exit could not fetch the live message; leaving it as it is");
            return;
        };
        if root_back {
            session_exit::dismiss_root_view(&self.ctx, reply, &message).await;
            return;
        }
        if let Err(error) = session_exit::handoff_to_hub(&self.ctx, &message).await {
            warn!("hub handoff failed ({error}); dismissing the view instead");
            session_exit::dismiss_root_view(&self.ctx, reply, &message).await;
        }
    }

    /// Starts the navigation loop with an initial destination.
    ///
    /// The loop runs one handler per popped [`Navigation`] until the session
    /// ends. `history` holds the frames open on the message: every target
    /// that runs becomes the newest frame, and a [`Navigation::Back`] marker
    /// closes the newest frame and re-runs the one revealed beneath it — so
    /// Back pops exactly one level, and a second Back pops one more.
    ///
    /// The two ways a session ends while the message still carries a Back
    /// button are the terminal steps: the hub handoff morphs the live
    /// message into the settings plugin's hub view and ends, and the root
    /// dismissal deletes the root view so no dead button stays behind. See
    /// [`crate::bot::command::session_exit`].
    pub async fn run(self: Arc<Self>, initial: Navigation) -> Result<(), Error> {
        self.navigate(initial).await;
        let mut history: VecDeque<Navigation> = VecDeque::new();
        loop {
            let popped = {
                let mut queue = self.nav_queue.lock().await;
                pop_step(&mut queue, &mut history)
            };
            match popped {
                Popped::Run(target) => {
                    let Some(mut handler) = self.handler_for(target) else {
                        return Ok(());
                    };
                    handler.run(self.clone()).await?;
                }
                Popped::HubHandoff => {
                    self.exit_through_reply(false).await;
                    return Ok(());
                }
                Popped::RootBack => {
                    self.exit_through_reply(true).await;
                    return Ok(());
                }
                Popped::End => return Ok(()),
            }
        }
    }
}

/// What the session loop does with the popped navigation state.
enum Popped {
    /// Run the handler for this target: it is the newest open frame.
    Run(Navigation),
    /// Morph the live message into the settings plugin's hub view and end the
    /// host session: the message continues as a plugin view session.
    HubHandoff,
    /// Back was pressed with no frame left beneath the one it closed: the
    /// message shows the root view, so it is dismissed.
    RootBack,
    /// End the session, leaving the message as it is.
    End,
}

/// Pops the next step off the pending queue against the open frames.
///
/// A plain target becomes the newest open frame and runs. A
/// [`Navigation::Back`] marker closes the newest frame, and the frame
/// revealed beneath it runs again — it stays the current frame, so the next
/// Back closes one more level. A Back that leaves no frame open is the root
/// view's Back. [`Navigation::SettingsMain`] is the hub handoff;
/// [`Navigation::Exit`] and an empty queue end the session. History is capped
/// at [`MAX_NAV_HISTORY`] frames, dropping the oldest.
fn pop_step(queue: &mut VecDeque<Navigation>, history: &mut VecDeque<Navigation>) -> Popped {
    let Some(target) = queue.pop_back() else {
        return Popped::End;
    };
    match target {
        Navigation::Exit => Popped::End,
        Navigation::SettingsMain => Popped::HubHandoff,
        Navigation::Back => {
            history.pop_back();
            match history.back().cloned() {
                Some(parent) => Popped::Run(parent),
                None => Popped::RootBack,
            }
        }
        target => {
            if history.len() >= MAX_NAV_HISTORY {
                history.pop_front();
            }
            history.push_back(target.clone());
            Popped::Run(target)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives [`pop_step`] the way a session drives it: `run` pushes the
    /// initial target, and each handler pushes the navigation it exits with
    /// before the loop pops the next step. The queue is a stack, so a
    /// pre-filled list would pop in reverse of the clicks.
    struct Walk {
        queue: VecDeque<Navigation>,
        history: VecDeque<Navigation>,
    }

    impl Walk {
        fn new(initial: Navigation) -> Self {
            Self {
                queue: VecDeque::from([initial]),
                history: VecDeque::new(),
            }
        }

        /// Pops the step the loop takes for the navigation the session just
        /// pushed.
        fn pushed(&mut self, next: Navigation) -> Popped {
            self.queue.push_back(next);
            pop_step(&mut self.queue, &mut self.history)
        }

        /// The frame on screen was opened by the initial navigation.
        fn opened(&mut self) -> Popped {
            pop_step(&mut self.queue, &mut self.history)
        }

        /// The Back button on the frame on screen was clicked.
        fn back(&mut self) -> Popped {
            self.pushed(Navigation::Back)
        }
    }

    /// Direct `/about`: About is the only frame open, so the Back its handler
    /// pushes leaves nothing beneath it — the root dismissal.
    #[test]
    fn back_over_the_only_frame_is_the_root_back() {
        let mut walk = Walk::new(Navigation::SettingsAbout);
        assert!(matches!(
            walk.opened(),
            Popped::Run(Navigation::SettingsAbout)
        ));
        assert!(matches!(walk.back(), Popped::RootBack));
    }

    /// Feed list → About → Back: the marker closes the About frame and
    /// the parent beneath it runs again, morphing the same message back.
    #[test]
    fn back_pops_one_level_to_the_parent() {
        let mut walk = Walk::new(Navigation::FeedList(None));
        assert!(matches!(
            walk.opened(),
            Popped::Run(Navigation::FeedList(None))
        ));
        assert!(matches!(
            walk.pushed(Navigation::SettingsAbout),
            Popped::Run(Navigation::SettingsAbout)
        ));
        assert!(matches!(
            walk.back(),
            Popped::Run(Navigation::FeedList(None))
        ));
    }

    /// The parent a Back reveals stays the current frame rather than being
    /// re-pushed, so the next Back closes it too: two Backs from About leave
    /// the session instead of looping on the parent.
    #[test]
    fn a_revealed_parent_stays_the_current_frame() {
        let mut walk = Walk::new(Navigation::FeedList(None));
        walk.opened();
        walk.pushed(Navigation::SettingsAbout);
        assert!(matches!(
            walk.back(),
            Popped::Run(Navigation::FeedList(None))
        ));
        assert_eq!(
            walk.history.back(),
            Some(&Navigation::FeedList(None)),
            "the revealed parent is still the current frame"
        );
        assert!(matches!(walk.back(), Popped::RootBack));
    }

    /// Three frames deep, each Back closes exactly one: the walk uncovers the
    /// frames in order rather than collapsing to the root.
    #[test]
    fn consecutive_backs_each_close_one_frame() {
        let mut walk = Walk::new(Navigation::SettingsAbout);
        walk.opened();
        walk.pushed(Navigation::FeedSubscribe {
            links: "a".into(),
            send_into: None,
        });
        walk.pushed(Navigation::FeedUnsubscribe {
            links: "b".into(),
            send_into: None,
        });
        assert!(matches!(
            walk.back(),
            Popped::Run(Navigation::FeedSubscribe {
                links: _,
                send_into: _,
            })
        ));
        assert!(matches!(
            walk.back(),
            Popped::Run(Navigation::SettingsAbout)
        ));
    }

    /// Exiting to the hub is the handoff, wherever in the walk it happens:
    /// the message morphs into the plugin hub and the host run ends.
    #[test]
    fn settings_main_is_the_hub_handoff() {
        let mut walk = Walk::new(Navigation::SettingsAbout);
        walk.opened();
        assert!(matches!(
            walk.pushed(Navigation::SettingsMain),
            Popped::HubHandoff
        ));
    }

    /// A session that ends on its own — `Exit`, or a handler that returns
    /// without pushing anything (the timeout) — ends without dismissing: the
    /// message the user is looking at stays up.
    #[test]
    fn ending_a_session_never_dismisses_the_message() {
        let mut walk = Walk::new(Navigation::SettingsAbout);
        walk.opened();
        assert!(matches!(walk.pushed(Navigation::Exit), Popped::End));

        let mut idle = Walk::new(Navigation::FeedList(None));
        idle.opened();
        assert!(matches!(
            pop_step(&mut idle.queue, &mut idle.history),
            Popped::End
        ));
        assert_eq!(idle.history.len(), 1, "the open frame is left alone");
    }

    /// History is capped: the oldest frame falls off, so a session that keeps
    /// navigating deeper cannot grow the walk without bound.
    #[test]
    fn history_is_capped_at_max_nav_history() {
        let mut walk = Walk::new(Navigation::SettingsAbout);
        walk.opened();
        for step in 0..MAX_NAV_HISTORY {
            walk.pushed(Navigation::FeedSubscribe {
                links: step.to_string(),
                send_into: None,
            });
        }
        assert_eq!(walk.history.len(), MAX_NAV_HISTORY);
        assert_eq!(
            walk.history.front(),
            Some(&Navigation::FeedSubscribe {
                links: "0".into(),
                send_into: None,
            }),
            "the initial frame is the oldest and the first to fall off"
        );
    }

    /// The Back marker never reaches the handler map: `pop_step` resolves it
    /// into a parent, the root dismissal, or the end.
    #[test]
    fn a_back_marker_never_becomes_a_runnable_target() {
        let mut walk = Walk::new(Navigation::SettingsAbout);
        walk.opened();
        assert!(!matches!(walk.back(), Popped::Run(_)));
    }
}
