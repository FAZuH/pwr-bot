use governor::RateLimiter;
use governor::clock::QuantaClock;
use governor::state::InMemoryState;
use governor::state::direct::NotKeyed;
use log::debug;
use log::info;
use serde::de::DeserializeOwned;

use crate::feed::error::FeedError;

pub mod anilist;
pub mod comick;
pub mod mangadex;
pub mod platforms;

pub use anilist::AniListPlatform;
pub use comick::ComickPlatform;
pub use mangadex::MangaDexPlatform;
pub use platforms::Platforms;

/// Sends a rate-limited request and deserializes the JSON response body.
///
/// `check_errors` validates the parsed response before it is returned.
pub(crate) async fn send_json<T, F>(
    client: &wreq::Client,
    limiter: &RateLimiter<NotKeyed, InMemoryState, QuantaClock>,
    source_name: &str,
    request: wreq::RequestBuilder,
    check_errors: F,
) -> Result<T, FeedError>
where
    T: DeserializeOwned,
    F: FnOnce(&T) -> Result<(), FeedError>,
{
    if limiter.check().is_err() {
        info!("Source {source_name} is ratelimited. Waiting...");
    }
    limiter.until_ready().await;

    let req = request.build()?;
    debug!("Making request to: {}", req.url());
    let response = client.execute(req).await?;

    let body = response.text().await?;
    let resp: T = serde_json::from_str(&body)?;
    check_errors(&resp)?;
    Ok(resp)
}
