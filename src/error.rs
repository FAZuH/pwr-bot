//! Top-level application errors.

use std::fmt::Display;

use log::error;
use uuid::Uuid;

use crate::database::error::DatabaseError;
use crate::feed::error::FeedError;
use crate::service::error::ServiceError;

/// Application-level errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AppError {
    #[error("Something went wrong on our end. Please try again later.")]
    InternalError,

    #[error("Something went wrong on our end. Reference: {ref_id}")]
    InternalWithRef { ref_id: Uuid },

    #[error("Missing configuration \"{config}\"")]
    MissingConfig { config: String },

    #[error("Error in app configuration: {msg}")]
    ConfigurationError { msg: String },
}

impl AppError {
    /// Log details internally, return generic error to user
    pub fn internal_with_ref(msg: impl Display) -> Self {
        let ref_id = Uuid::new_v4();
        error!("Internal error ({ref_id}): {msg}");
        Self::InternalWithRef { ref_id }
    }
}

/// Union of all possible error types in the application.
pub enum AppErrorKind {
    AppError(AppError),
    DatabaseError(DatabaseError),
    FeedError(FeedError),
    ServiceError(ServiceError),
}
