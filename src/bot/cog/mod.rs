use crate::bot::Data;

pub mod admin_cog;
pub mod feeds_cog;
pub mod owner_cog;
pub mod voice_cog;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

pub use admin_cog::AdminCog;
pub use feeds_cog::FeedsCog;
pub use owner_cog::OwnerCog;
pub use voice_cog::VoiceCog;
