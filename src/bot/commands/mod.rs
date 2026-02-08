use crate::bot::Data;

pub mod admin_cog;
pub mod feeds;
pub mod owner_cog;
pub mod voice;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

pub use admin_cog::AdminCog;
pub use feeds::FeedsCog;
pub use owner_cog::OwnerCog;
use poise::Command;
pub use voice::VoiceCog;

pub trait Cog {
    fn commands(&self) -> Vec<Command<Data, Error>>;
}

pub struct Cogs;

impl Cog for Cogs {
    fn commands(&self) -> Vec<Command<Data, Error>> {
        let feeds_cog = FeedsCog;
        let admin_cog = AdminCog;
        let owner_cog = OwnerCog;
        let voice_cog = VoiceCog;

        feeds_cog
            .commands()
            .into_iter()
            .chain(admin_cog.commands())
            .chain(owner_cog.commands())
            .chain(voice_cog.commands())
            .collect()
    }
}
