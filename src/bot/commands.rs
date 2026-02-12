//! Bot command organization using the Cog pattern.

use crate::bot::Data;

pub mod about;
pub mod admin;
pub mod feed;
pub mod owner;
pub mod voice;
pub mod settings;

/// Error type used across bot commands.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// Context type passed to command handlers.
pub type Context<'a> = poise::Context<'a, Data, Error>;

pub use about::AboutCog;
pub use admin::AdminCog;
pub use feed::FeedCog;
pub use owner::OwnerCog;
use poise::Command;
pub use voice::VoiceCog;

/// Trait for command modules that provide Discord commands.
pub trait Cog {
    /// Returns the list of commands provided by this cog.
    fn commands(&self) -> Vec<Command<Data, Error>>;
}

/// Aggregates all command cogs.
pub struct Cogs;

impl Cog for Cogs {
    fn commands(&self) -> Vec<Command<Data, Error>> {
        let feeds_cog = FeedCog;
        let admin_cog = AdminCog;
        let owner_cog = OwnerCog;
        let voice_cog = VoiceCog;
        let about_cog = AboutCog;

        feeds_cog
            .commands()
            .into_iter()
            .chain(admin_cog.commands())
            .chain(owner_cog.commands())
            .chain(voice_cog.commands())
            .chain(about_cog.commands())
            .collect()
    }
}
