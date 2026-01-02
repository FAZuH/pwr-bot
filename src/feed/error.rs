#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UrlParseError {
    #[error("The site `{site}` is not supported.")]
    UnsupportedSite { site: String },

    #[error("The URL `{url}` has an invalid format.")]
    InvalidFormat { url: String },

    #[error("Could not find an identifier in the URL `{url}`.")]
    MissingId { url: String },
}

#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Failed to parse API response: {0}")]
    JsonParseFailed(#[from] serde_json::Error),

    #[error("Feed source not found (ID: {source_id}).")]
    SourceNotFound { source_id: String },

    #[error("Latest item not found for feed (ID: {source_id}).")]
    ItemNotFound { source_id: String },

    #[error("Feed is finished (ID: {source_id}).")]
    SourceFinished { source_id: String },

    #[error("Feed source contains no items (ID: {source_id}).")]
    EmptySource { source_id: String },

    #[error("Invalid data from API: missing field `{field}`.")]
    MissingField { field: String },

    #[error("Invalid source ID: {source_id}.")]
    InvalidSourceId { source_id: String },

    #[error("Feed API error: {message}")]
    ApiError { message: String },

    #[error("Invalid timestamp received: {timestamp}.")]
    InvalidTimestamp { timestamp: i64 },

    #[error("Invalid time format received: {time}.")]
    InvalidTime { time: String },

    #[error("The URL `{url}` is not supported.")]
    UnsupportedUrl { url: String },

    #[error("Unexpected error: {message}")]
    UnexpectedResult { message: String },

    #[error(transparent)]
    UrlParseFailed(#[from] UrlParseError),
}

impl From<reqwest::Error> for FeedError {
    fn from(e: reqwest::Error) -> Self {
        FeedError::RequestFailed(Box::new(e))
    }
}

impl From<wreq::Error> for FeedError {
    fn from(e: wreq::Error) -> Self {
        FeedError::RequestFailed(Box::new(e))
    }
}
