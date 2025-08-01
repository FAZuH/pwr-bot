pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

impl Error {
    pub fn new(kind: ErrorKind, message: String) -> Self {
        Self { kind, message }
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UrlParseError {
    #[error("Unsupported site: {site}. Only mangadex.org URLs are supported")]
    UnsupportedSite { site: String },

    #[error("Invalid URL format: {url}. Expected format: https://mangadex.org/[type]/[id]")]
    InvalidFormat { url: String },

    #[error("Missing identifier in URL: {url}")]
    MissingId { url: String },
    // #[error("Invalid URL scheme: {scheme}. Only http and https are supported")]
    // InvalidScheme { scheme: String },
    // #[error("Malformed URL: {url}")]
    // MalformedUrl { url: String },
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    #[error("Failed to parse JSON response: {0}")]
    JsonParseFailed(#[from] serde_json::Error),

    #[error("Series not found with ID: {series_id}")]
    SeriesNotFound { series_id: String },

    #[error("Series finished for series ID: {series_id}")]
    FinishedSeries { series_id: String },

    #[error("Empty series for series ID: {series_id}")]
    EmptySeries { series_id: String },

    #[error("Invalid or missing data in API response: {field}")]
    MissingField { field: String },

    #[error("Invalid series ID format: {series_id}")]
    InvalidSeriesId { series_id: String },

    #[error("API returned error: {message}")]
    ApiError { message: String },

    #[error("Invalid timestamp in response: {timestamp}")]
    InvalidTimestamp { timestamp: i64 },

    #[error("Invalid time in response: {time}")]
    InvalidTime { time: String },

    #[error("Unsupported url: {url}")]
    UnsupportedUrl { url: String },

    #[error("Wrong result type when downcasting")]
    WrongResultType,

    #[error("URL parse error: {0}")]
    UrlParseFailed(#[from] UrlParseError),
}

pub enum ErrorKind {
    SourceError(SourceError),
    UrlParseError(UrlParseError),
}
