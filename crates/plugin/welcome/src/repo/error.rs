#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DatabaseError {
    #[error("Database error: {0}")]
    BackendError(#[from] diesel::result::Error),

    #[error("Data parse error: {message}")]
    ParseError { message: String },

    #[error("Async join error: {0}")]
    JoinError(String),

    #[error("Pool error: {0}")]
    PoolError(String),
}

impl From<tokio::task::JoinError> for DatabaseError {
    fn from(value: tokio::task::JoinError) -> Self {
        Self::JoinError(value.to_string())
    }
}

impl From<diesel_async::pooled_connection::deadpool::PoolError> for DatabaseError {
    fn from(value: diesel_async::pooled_connection::deadpool::PoolError) -> Self {
        Self::PoolError(value.to_string())
    }
}
