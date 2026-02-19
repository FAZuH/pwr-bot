//! Database-specific error types.

use crate::error::AppError;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DatabaseError {
    /// Error from the underlying database backend (sqlx)
    #[error("Database error: {0}")]
    BackendError(#[from] sqlx::Error),

    /// Failed to parse or extract data from a model field
    #[error("Data parse error: {message}")]
    ParseError { message: String },

    #[error(transparent)]
    AppError(#[from] AppError),
}
