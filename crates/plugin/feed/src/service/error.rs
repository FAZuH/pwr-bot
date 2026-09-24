use crate::feed::error::FeedError;
use crate::repo::error::DatabaseError;

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
