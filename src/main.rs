pub mod action;
pub mod config;
pub mod event;
pub mod listener;
pub mod source;
pub mod bot;

use crate::config::Config;
use crate::listener::{Listener, PollingListener};
use crate::source::{MangaDexSource, AniListSource};
use crate::bot::{Handler};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::new();

    // Setup listener
    let mut listener = PollingListener::new(config.clone()).await?;
    listener.register_source("manga".to_string(), Arc::new(MangaDexSource::new(&config)));
    listener.register_source("anime".to_string(), Arc::new(AniListSource::new(&config)));
    listener.start()?;

    // Setup Discord bot

    // Listen for exit signal
    tokio::signal::ctrl_c().await?;
    listener.stop()?;
    Ok(())
}

