pub mod feeds_cog;
pub mod owner_cog;

use crate::bot::Data;
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;
