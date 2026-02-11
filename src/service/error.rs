//! Service-level error types.

use crate::database::error::DatabaseError;
use crate::feed::error::FeedError;

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
