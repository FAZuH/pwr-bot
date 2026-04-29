//! Database-specific error types.

use crate::error::AppError;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DatabaseError {
    /// Error from the underlying database backend (Diesel)
    #[error("Database error: {0}")]
    BackendError(#[from] diesel::result::Error),

    /// Failed to parse or extract data from a model field
    #[error("Data parse error: {message}")]
    ParseError { message: String },

    #[error(transparent)]
    AppError(#[from] AppError),

    /// Async task join error
    #[error("Async join error: {0}")]
    JoinError(String),

    /// Connection pool error
    #[error("Pool error: {0}")]
    PoolError(String),
}

impl From<tokio::task::JoinError> for DatabaseError {
    fn from(value: tokio::task::JoinError) -> Self {
        DatabaseError::JoinError(value.to_string())
    }
}

impl From<diesel_async::pooled_connection::deadpool::PoolError> for DatabaseError {
    fn from(value: diesel_async::pooled_connection::deadpool::PoolError) -> Self {
        DatabaseError::PoolError(value.to_string())
    }
}
