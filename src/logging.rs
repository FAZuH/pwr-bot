//! Logging setup and configuration.

pub use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::RollingFileAppender;
use tracing_appender::rolling::Rotation;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::Config;
use crate::error::AppError;

/// Sets up logging with both console and file output. Dropping the returned
/// guard flushes pending log lines; keep it alive for the process lifetime.
pub fn setup_logging(config: &Config) -> Result<WorkerGuard, AppError> {
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("pwr-bot")
        .filename_suffix("log")
        .max_log_files(7)
        .latest_symlink("pwr-bot.latest.log")
        .build(&config.logs_path)
        .map_err(|e| AppError::ConfigurationError {
            msg: format!(
                "Failed to initialize rolling file appender at '{}': {}",
                config.logs_path.to_string_lossy(),
                e
            ),
        })?;

    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("pwr_bot=info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stdout).with_ansi(true))
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();

    Ok(guard)
}
