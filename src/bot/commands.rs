//! Bot command organization using the Cog pattern.

pub mod about;
pub mod dump_db;
pub mod feed;
pub mod register;
pub mod register_owner;
pub mod settings;
pub mod unregister;
pub mod voice;
pub mod welcome;

/// Error type used across bot commands.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Context type passed to command handlers.
pub type Context<'a> = poise::Context<'a, Data, Error>;

use poise::Command;

use crate::bot::Data;

/// Trait for command modules that provide Discord commands.
pub trait Cog {
    /// Returns the list of commands provided by this cog.
    fn commands(&self) -> Vec<Command<Data, Error>>;
}

/// Aggregates all command cogs.
pub struct Cogs;

impl Cog for Cogs {
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
