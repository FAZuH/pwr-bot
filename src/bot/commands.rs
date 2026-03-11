//! Bot command organization using the Cog pattern.
//!
//! This module provides a way to group and aggregate Discord commands using the
//! [`Cog`] trait. This structure allows for modular command definitions across
//! different files and domains.

pub mod about;
pub mod dump_db;
pub mod feed;
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

use poise::Command;

use crate::bot::Data;

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
            register::register(),
            register_owner::register_owner(),
            settings::settings(),
            unregister::unregister(),
            voice::voice(),
            welcome::welcome(),
        ]
    }
}
