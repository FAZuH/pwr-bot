//! Utility functions for bot commands.

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
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        let hours = seconds / 3600;
        let mins = (seconds % 3600) / 60;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    } else {
        let days = seconds / 86400;
        let hours = (seconds % 86400) / 3600;
        if hours > 0 {
            format!("{}d {}h", days, hours)
        } else {
            format!("{}d", days)
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
                "Too many links provided. Please provide no more than {} links at a time.",
                MAX_URLS_PER_REQUEST
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_urls_accepts_valid_count() {
        let urls = vec!["url1", "url2", "url3"];
        assert!(validate_url_count(&urls).is_ok());
    }

    #[test]
    fn test_validate_urls_rejects_too_many() {
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
    fn test_validate_urls_accepts_exactly_ten() {
        let urls = vec!["url"; 10];
        assert!(validate_url_count(&urls).is_ok());
    }

    #[test]
    fn test_parse_and_validate_splits_comma_separated() {
        let input = "url1, url2 ,url3";
        let urls = parse_and_validate_urls(input).unwrap();
        assert_eq!(urls, vec!["url1", "url2", "url3"]);
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(3599), "59m");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3660), "1h 1m");
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(86399), "23h 59m");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90000), "1d 1h");
        assert_eq!(format_duration(172800), "2d");
        assert_eq!(format_duration(604800), "7d");
    }

    #[test]
    fn test_format_duration_large_values() {
        assert_eq!(format_duration(8640000), "100d"); // 100 days exactly
        assert_eq!(format_duration(8640000 + 3600), "100d 1h"); // 100 days + 1 hour
    }
}
