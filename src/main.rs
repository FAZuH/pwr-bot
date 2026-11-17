//! Application entry point for pwr-bot.
//!
//! Initializes the host database, starts the bot, and lets internal plugins own
//! their domains.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use dotenv::dotenv;
use log::debug;
use log::info;
use pwr_bot::bot::Bot;
use pwr_bot::cli::Cli;
use pwr_bot::cli::Command;
use pwr_bot::cli::DebugCommand;
use pwr_bot::cli::log_tail;
use pwr_bot::cli::service;
use pwr_bot::config::Config;
use pwr_bot::event::event_bus::EventBus;
use pwr_bot::logging::WorkerGuard;
use pwr_bot::logging::setup_logging;
use pwr_bot::repo::PgRepos;
use pwr_bot::repo::traits::Repos;
use pwr_bot::service::Services;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Serve {
            service: service_mode,
        }) => serve(service_mode).await,
        Some(Command::Plugin { action }) => pwr_bot::cli::plugin::run(action).await,
        Some(Command::Service { action }) => match action {
            service::ServiceCommand::Start => service::start().await,
            service::ServiceCommand::Stop => service::stop().await,
            service::ServiceCommand::Restart => service::restart().await,
            service::ServiceCommand::Status => service::status(),
            service::ServiceCommand::Install { env_file } => service::install(&env_file),
        },
        Some(Command::Version) => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(Command::Debug { target }) => match target {
            DebugCommand::Paths => {
                service::print_paths();
                Ok(())
            }
        },
        None => {
            service::ensure().await?;
            log_tail::follow(&log_tail::log_file_path(), cli.lines).await
        }
    }
}

async fn serve(service_mode: bool) -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("no other rustls CryptoProvider installed yet");

    let init_start = Instant::now();
    let (config, guard) = load_config().await?;
    let event_bus = Arc::new(EventBus::new());
    let repos = setup_database(&config, init_start).await?;
    let services = setup_services(repos.clone()).await?;
    setup_bot(&config, event_bus, services, repos, init_start).await?;
    info!(
        "pwr-bot is up in {:.2}s. Press Ctrl+C to stop.",
        init_start.elapsed().as_secs_f64()
    );

    let registration = if service_mode {
        let reg = service::new_registration(&config.logs_path);
        service::write_registration(&service::registration_path(), &reg)
            .context("failed to register the service")?;
        Some(reg)
    } else {
        None
    };

    let reason = match registration {
        Some(reg) => {
            let reason = tokio::select! {
                reason = service::wait_for_signal() => reason,
                _ = service::watchdog(reg.clone()) => "service lease changed",
            };
            service::release(&reg);
            reason
        }
        None => service::wait_for_signal().await,
    };
    info!("{reason}, shutting down.");
    drop(guard);
    Ok(())
}

async fn load_config() -> Result<(Arc<Config>, WorkerGuard)> {
    debug!("Loading configuration...");
    let mut config = Config::new();
    config.load()?;
    let guard = setup_logging(&config)?;
    let config = Arc::new(config);
    info!("Starting pwr-bot...");
    Ok((config, guard))
}

async fn setup_database(
    config: &Config,
    init_start: Instant,
) -> Result<Arc<dyn Repos + Send + Sync>> {
    debug!("Setting up Database...");
    let repos = PgRepos::new(&config.db_url).await?;
    info!("Running database migrations...");
    repos.run_migrations().await?;
    info!(
        "Database setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );
    Ok(Arc::new(repos))
}

async fn setup_services(repos: Arc<dyn Repos + Send + Sync>) -> Result<Arc<Services>> {
    debug!("Setting up Services...");
    Ok(Arc::new(Services::new(repos).await?))
}

async fn setup_bot(
    config: &Arc<Config>,
    event_bus: Arc<EventBus>,
    services: Arc<Services>,
    repos: Arc<dyn Repos + Send + Sync>,
    init_start: Instant,
) -> Result<Arc<Bot>> {
    info!("Starting bot...");
    let mut bot = Bot::new(config.clone(), event_bus, services, repos).await?;
    bot.start();
    let bot = Arc::new(bot);
    info!(
        "Bot setup complete ({:.2}s).",
        init_start.elapsed().as_secs_f64()
    );
    Ok(bot)
}
