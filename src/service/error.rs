//! Service-level error types.

use crate::feed::error::FeedError;
use crate::repository::error::DatabaseError;

/// Errors that can occur in service operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServiceError {
    #[error("Unexpected result: {message}")]
    UnexpectedResult { message: String },

    #[error(transparent)]
    FeedError(#[from] FeedError),

    #[error(transparent)]
    DatabaseError(#[from] DatabaseError),
}
