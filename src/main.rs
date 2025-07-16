pub mod action;
pub mod config;
pub mod event;
pub mod source;
pub mod bot;

use crate::config::Config;
use crate::source::manga_dex_source::MangaDexSource;
use crate::source::ani_list_source::AniListSource;
use crate::source::source::Source;
use crate::event::update_publisher::UpdatePublisher;
use crate::bot::{Handler};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::new();

    // Setup listener
    let mut publisher = UpdatePublisher::new(config.clone()).await?;
    publisher.register_source(Source::Manga(MangaDexSource::new(&config)));
    publisher.register_source(Source::Anime(AniListSource::new(&config)));
    publisher.start()?;

    // Setup Discord bot

    // Listen for exit signal
    tokio::signal::ctrl_c().await?;
    publisher.stop()?;
    Ok(())
}

