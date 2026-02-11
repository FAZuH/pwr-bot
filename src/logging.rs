//! Logging setup and configuration.

use tracing_appender::rolling::RollingFileAppender;
use tracing_appender::rolling::Rotation;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::error::AppError;

/// Sets up logging with both console and file output.
pub fn setup_logging(config: &Config) -> Result<(), AppError> {
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("pwr-bot")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&config.logs_path)
        .map_err(|e| AppError::ConfigurationError {
            msg: format!(
                "Failed to initialize rolling file appender at '{}': {}",
                config.logs_path.to_string_lossy(),
                e
            ),
        })?;

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Leak the guard to prevent it from being dropped
    std::mem::forget(_guard);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("pwr_bot=info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stdout).with_ansi(true))
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();

    Ok(())
}
