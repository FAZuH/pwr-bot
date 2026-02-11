//! Top-level application errors.

use crate::database::error::DatabaseError;
use crate::feed::error::FeedError;

/// Application-level errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AppError {
    #[error("Assertion error: {msg}")]
    AssertionError { msg: String },

    #[error("Missing config \"{config}\"")]
    MissingConfig { config: String },

    #[error("Error in app configuration: {msg}")]
    ConfigurationError { msg: String },
}

/// Union of all possible error types in the application.
pub enum AppErrorKind {
    AppError(AppError),
    DatabaseError(DatabaseError),
    FeedError(FeedError),
}
