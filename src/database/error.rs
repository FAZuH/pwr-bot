#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DatabaseError {
    #[error("Internal database error: {0}")]
    BackendError(#[from] sqlx::Error),

    #[error("Internal database error: {message}")]
    InternalError { message: String },
}
