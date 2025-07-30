#[derive(Debug)]
#[non_exhaustive]
pub enum UrlParseError {
    UnsupportedSite,
    InvalidFormat,
    MissingId,
}
