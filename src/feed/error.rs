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

    #[error("Series not found with ID: {series_id}")]
    SeriesItemNotFound { series_id: String },

    #[error("Latest item not found for series with ID: {series_id}")]
    SeriesLatestNotFound { series_id: String },

    #[error("Series finished for series ID: {series_id}")]
    FinishedSeries { series_id: String },

    #[error("Empty series for series ID: {series_id}")]
    EmptySeries { series_id: String },

    #[error("Invalid or missing data in API response: {field}")]
    MissingField { field: String },

    #[error("Invalid series ID: {series_id}")]
    InvalidSeriesId { series_id: String },

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
