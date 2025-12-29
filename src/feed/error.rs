#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UrlParseError {
    #[error("Unsupported site: {site}.")]
    UnsupportedSite { site: String },

    #[error("Invalid URL format: {url}.")]
    InvalidFormat { url: String },

    #[error("Missing identifier in URL: {url}")]
    MissingId { url: String },
    // #[error("Invalid URL scheme: {scheme}. Only http and https are supported")]
    // InvalidScheme { scheme: String },
    // #[error("Malformed URL: {url}")]
    // MalformedUrl { url: String },
}

#[derive(Debug, thiserror::Error)]
pub enum SeriesFeedError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    #[error("Failed to parse JSON response: {0}")]
    JsonParseFailed(#[from] serde_json::Error),

    #[error("Source not found with ID: {source_id}")]
    SourceNotFound { source_id: String },

    #[error("Latest item not found for source with ID: {source_id}")]
    ItemNotFound { source_id: String },

    #[error("Source finished for source ID: {source_id}")]
    SourceFinished { source_id: String },

    #[error("Empty source for source ID: {source_id}")]
    EmptySource { source_id: String },

    #[error("Invalid or missing data in API response: {field}")]
    MissingField { field: String },

    #[error("Invalid source ID: {source_id}")]
    InvalidSourceId { source_id: String },

    #[error("API returned error: {message}")]
    ApiError { message: String },

    #[error("Invalid timestamp in response: {timestamp}")]
    InvalidTimestamp { timestamp: i64 },

    #[error("Invalid time in response: {time}")]
    InvalidTime { time: String },

    #[error("Unsupported url: {url}")]
    UnsupportedUrl { url: String },

    #[error("Unexpected result: {message}")]
    UnexpectedResult { message: String },

    #[error("URL parse error: {0}")]
    UrlParseFailed(#[from] UrlParseError),
}

#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error("SeriesFeedError: {0}")]
    SeriesFeedError(#[from] SeriesFeedError),
}
