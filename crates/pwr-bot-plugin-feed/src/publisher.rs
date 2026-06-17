use deadpool_postgres::Pool;
use pwr_bot_sdk::*;

use crate::platform::Platforms;
use crate::subscription;

pub async fn poll_feeds(
    pool: &Pool,
    host: &PluginHost,
    platforms: &Platforms,
) -> Result<(), String> {
    let feeds = subscription::get_feeds_by_tag(pool, host, "series").await?;
    let len = feeds.len();
    if len == 0 {
        return Ok(());
    }

    for feed in &feeds {
        let name = feed.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        match subscription::check_feed_update(pool, host, platforms, feed).await {
            Ok(result) => {
                subscription::publish_update(host, &result).await;
            }
            Err(e) => {
                let id = feed.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                tracing::error!(feed.id = id, feed.name = %name, error = %e, "error checking feed");
            }
        }
    }

    Ok(())
}
