//! Service-level error types.

use crate::repo::error::DatabaseError;

/// Errors that can occur in service operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServiceError {
    #[error("Unexpected result: {message}")]
    UnexpectedResult { message: String },

    #[error(transparent)]
    DatabaseError(#[from] DatabaseError),
}
