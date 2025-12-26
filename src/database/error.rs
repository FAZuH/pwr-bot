#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DatabaseError {
    /// Error from the underlying database backend (sqlx)
    #[error("Internal database error: {0}")]
    BackendError(#[from] sqlx::Error),

    /// Internal database error not originating from the backend
    #[error("Internal database error: {message}")]
    InternalError { message: String },

    /// Failed to parse or extract data from a model field
    #[error("Parse error: {message}")]
    ParseError { message: String },
}
