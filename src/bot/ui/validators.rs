use crate::bot::error::BotError;

pub const MAX_URLS_PER_REQUEST: usize = 10;

pub fn parse_and_validate_urls(links: &str) -> Result<Vec<&str>, BotError> {
    let urls: Vec<&str> = links.split(',').map(|s| s.trim()).collect();
    validate_url_count(&urls)?;
    Ok(urls)
}

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
}
