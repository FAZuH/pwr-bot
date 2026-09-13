//! Utility functions for bot commands.

use std::io::Cursor;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::DynamicImage;
use image::imageops::FilterType;

use crate::bot::error::BotError;

/// Maximum number of URLs allowed per subscription request.
pub const MAX_URLS_PER_REQUEST: usize = 10;

/// Formats a duration in seconds into a human-readable string.
///
/// Examples:
/// - 30 -> "30s"
/// - 120 -> "2m"
/// - 3660 -> "1h 1m"
/// - 86400 -> "1d"
/// - 90000 -> "1d 1h"
pub fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        let hours = seconds / 3600;
        let mins = (seconds % 3600) / 60;
        if mins > 0 {
            format!("{hours}h {mins}m")
        } else {
            format!("{hours}h")
        }
    } else {
        let days = seconds / 86400;
        let hours = (seconds % 86400) / 3600;
        if hours > 0 {
            format!("{days}d {hours}h")
        } else {
            format!("{days}d")
        }
    }
}

/// Parses a comma-separated string of URLs and validates the count.
pub fn parse_and_validate_urls(links: &str) -> Result<Vec<&str>, BotError> {
    let urls: Vec<&str> = links.split(',').map(|s| s.trim()).collect();
    validate_url_count(&urls)?;
    Ok(urls)
}

/// Validates that the number of URLs does not exceed the maximum.
pub fn validate_url_count(urls: &[&str]) -> Result<(), BotError> {
    if urls.len() > MAX_URLS_PER_REQUEST {
        return Err(BotError::InvalidCommandArgument {
            parameter: "links".to_string(),
            reason: format!(
                "Too many links provided. Please provide no more than {MAX_URLS_PER_REQUEST} links at a time."
            ),
        });
    }
    Ok(())
}

/// Gets the current process memory usage in megabytes.
pub fn process_memory_mb() -> f64 {
    use sysinfo::System;
    use sysinfo::get_current_pid;

    let mut s = System::new_all();
    s.refresh_all();

    if let Ok(pid) = get_current_pid()
        && let Some(process) = s.process(pid)
    {
        return process.memory() as f64 / (1024.0 * 1024.0);
    }

    0.0
}

/// Downloads an avatar and returns it as a base64 PNG resized to `size` pixels.
pub async fn download_avatar(
    http_client: &wreq::Client,
    url: &str,
    size: u32,
) -> anyhow::Result<String> {
    let response = http_client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download avatar: {}",
            response.status()
        ));
    }
    let bytes = response.bytes().await?;
    let img = image::load_from_memory(&bytes)?;
    Ok(avatar_to_b64(&img, size))
}

/// Resizes an avatar to `size` pixels and encodes it as a base64 PNG.
pub fn avatar_to_b64(img: &DynamicImage, size: u32) -> String {
    let resized = img.resize_exact(size, size, FilterType::Lanczos3);
    let mut cursor = Cursor::new(Vec::new());
    resized
        .write_to(&mut cursor, image::ImageFormat::Png)
        .unwrap();
    BASE64.encode(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_urls_accepts_valid_count() {
        let urls = vec!["url1", "url2", "url3"];
        assert!(validate_url_count(&urls).is_ok());
    }

    #[test]
    fn validate_urls_rejects_too_many() {
        let urls = vec!["url"; 11];
        let result = validate_url_count(&urls);
        assert!(result.is_err());
        match result.unwrap_err() {
            BotError::InvalidCommandArgument { parameter, reason } => {
                assert_eq!(parameter, "links");
                assert!(reason.contains("no more than 10"));
            }
            _ => panic!("Expected InvalidCommandArgument error"),
        }
    }

    #[test]
    fn validate_urls_accepts_exactly_ten() {
        let urls = vec!["url"; 10];
        assert!(validate_url_count(&urls).is_ok());
    }

    #[test]
    fn parse_and_validate_splits_comma_separated() {
        let input = "url1, url2 ,url3";
        let urls = parse_and_validate_urls(input).unwrap();
        assert_eq!(urls, vec!["url1", "url2", "url3"]);
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(3599), "59m");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3660), "1h 1m");
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(86399), "23h 59m");
    }

    #[test]
    fn format_duration_days() {
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90000), "1d 1h");
        assert_eq!(format_duration(172800), "2d");
        assert_eq!(format_duration(604800), "7d");
    }

    #[test]
    fn format_duration_large_values() {
        assert_eq!(format_duration(8640000), "100d"); // 100 days exactly
        assert_eq!(format_duration(8640000 + 3600), "100d 1h"); // 100 days + 1 hour
    }
}
