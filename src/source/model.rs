use chrono::{DateTime, Utc};

#[derive(Clone, Debug)]
pub struct SeriesItem {
    pub id: String, // e.g. "1234567890"
    ///  Human readable title, e.g., "One Piece", "Attack on Titan".
    pub title: String,
    /// Latest chapter or episode title, e.g., "Chapter 100", "Episode 12".
    pub latest: String,
    /// Url of the series, e.g., "https://mangadex.org/title/1234567890"
    /// This is defined so that the source can be identified by notification receiver.
    pub url: String,
    /// Timestamp of the latest update, e.g., "2023-10-01T12:00:00Z".
    pub published: DateTime<Utc>,
}

#[non_exhaustive]
pub enum SourceResult {
    Series(SeriesItem),
}
