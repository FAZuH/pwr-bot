use std::hash::Hash;
use std::hash::Hasher;
use std::num::NonZeroU32;

use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;
use governor::Quota;
use governor::RateLimiter;
use governor::clock::QuantaClock;
use governor::state::InMemoryState;
use governor::state::direct::NotKeyed;
use log::debug;
use log::info;
use reqwest;
use reqwest::Client;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;
use reqwest::header::USER_AGENT;
use serde_json::Map;
use serde_json::Value;

use crate::feed::BasePlatform;
use crate::feed::FeedItem;
use crate::feed::FeedSource;
use crate::feed::Platform;
use crate::feed::PlatformInfo;
use crate::feed::error::FeedError;

type Json = Map<String, Value>;

pub struct ComickPlatform {
    pub base: BasePlatform,
    limiter: RateLimiter<NotKeyed, InMemoryState, QuantaClock>,
}

impl ComickPlatform {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("pwr-bot/0.1"));

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .expect("Failed to create client");

        let info = PlatformInfo {
            name: "Comick".to_string(),
            feed_item_name: "Chapter".to_string(),
            api_hostname: "api.comick.dev".to_string(),
            api_domain: "comick.dev".to_string(),
            api_url: "https://api.comick.dev".to_string(),
            copyright_notice: "© Comick 2021-2026".to_string(),
            // Discord doesn't support .svg files on their embed, and I can't find a .png link
            // under MangaDex's domain
            logo_url:
                "https://comick.dev/_next/image?url=%2Fstatic%2Ficons%2Funicorn-64.png&w=144&q=75"
                    .to_string(),
            tags: "series".to_string(),
        };

        // NOTE: Not documented, but we will use the ratelimit described in "x-ratelimit-limit" and
        // "x-ratelimit-reset" headers
        let limiter = RateLimiter::direct(Quota::per_minute(NonZeroU32::new(200).unwrap()));

        Self {
            base: BasePlatform::new(info, client),
            limiter,
        }
    }

    fn check_resp_errors(&self, resp: &Json) -> Result<(), FeedError> {
        if resp.get("statusCode").is_some() {
            return Err(FeedError::ApiError {
                message: resp
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error")
                    .to_string(),
            });
        }
        Ok(())
    }

    fn get_comic_from_resp<'a>(&self, resp: &'a Json) -> Result<&'a Json, FeedError> {
        resp.get("comic")
            .ok_or_else(|| FeedError::MissingField {
                field: "comic".to_string(),
            })?
            .as_object()
            .ok_or(FeedError::UnexpectedResult {
                message: "Failed converting comic field to JSON".to_string(),
            })
    }

    fn get_hid(&self, comic: &Json) -> Result<String, FeedError> {
        Ok(comic
            .get("hid")
            .ok_or_else(|| FeedError::MissingField {
                field: "comic.hid".to_string(),
            })?
            .as_str()
            .ok_or_else(|| FeedError::UnexpectedResult {
                message: "Failed converting comic.hid to string".to_string(),
            })?
            .to_string())
    }

    fn get_title(&self, comic: &Json) -> Result<String, FeedError> {
        Ok(comic
            .get("title")
            .ok_or_else(|| FeedError::MissingField {
                field: "comic.title".to_string(),
            })?
            .as_str()
            .ok_or_else(|| FeedError::UnexpectedResult {
                message: "Failed converting comic.title to string".to_string(),
            })?
            .to_string())
    }

    fn get_description(&self, comic: &Json) -> Result<String, FeedError> {
        Ok(comic
            .get("desc")
            .ok_or_else(|| FeedError::MissingField {
                field: "comic.desc".to_string(),
            })?
            .as_str()
            .ok_or_else(|| FeedError::UnexpectedResult {
                message: "Failed converting comic.desc to string".to_string(),
            })?
            .to_string())
    }

    fn get_cover_url(&self, comic: &Json) -> Result<String, FeedError> {
        let cover_filename = comic
            .get("md_covers")
            .ok_or_else(|| FeedError::MissingField {
                field: "comic.md_covers".to_string(),
            })?
            .as_array()
            .ok_or_else(|| FeedError::UnexpectedResult {
                message: "Failed converting comic.md_covers to array".to_string(),
            })?
            .first()
            .ok_or_else(|| FeedError::MissingField {
                field: "comic.md_covers.0".to_string(),
            })?
            .as_object()
            .ok_or_else(|| FeedError::UnexpectedResult {
                message: "Failed converting comic.md_covers.0 to JSON".to_string(),
            })?
            .get("b2key")
            .ok_or_else(|| FeedError::MissingField {
                field: "comic.md_covers.0.b2key".to_string(),
            })?
            .as_str()
            .ok_or_else(|| FeedError::UnexpectedResult {
                message: "Failed converting comic.md_covers.0.b2key to string".to_string(),
            })?;

        Ok(format!("https://meo.comick.pictures/{cover_filename}"))
    }

    fn get_latest_chapter<'a>(&self, resp: &'a Json, hid: &'a str) -> Result<&'a Json, FeedError> {
        resp.get("chapters")
            .ok_or_else(|| FeedError::MissingField {
                field: "chapters".to_string(),
            })?
            .as_array()
            .ok_or_else(|| FeedError::UnexpectedResult {
                message: "Failed converting chapters to array".to_string(),
            })?
            .first()
            .ok_or_else(|| FeedError::ItemNotFound {
                source_id: hid.to_string(),
            })?
            .as_object()
            .ok_or_else(|| FeedError::UnexpectedResult {
                message: "Failed converting chapters.0 to JSON".to_string(),
            })
    }

    fn get_chapter(&self, chapter: &Json) -> Result<String, FeedError> {
        Ok(chapter
            .get("chap")
            .ok_or_else(|| FeedError::MissingField {
                field: "chapters.0.chap".to_string(),
            })?
            .as_str()
            .ok_or_else(|| FeedError::UnexpectedResult {
                message: "Failed converting chapters.0.chap to string".to_string(),
            })?
            .to_string())
    }

    fn get_publish_at(&self, chapter: &Json) -> Result<DateTime<Utc>, FeedError> {
        chapter
            .get("publish_at")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FeedError::MissingField {
                field: "chapters.0.publish_at".to_string(),
            })?
            .parse()
            .map_err(|_| FeedError::UnexpectedResult {
                message: "API returned invalid format of chapters.0.publish_at".to_string(),
            })
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, reqwest::Error> {
        if self.limiter.check().is_err() {
            info!("Source {} is ratelimited. Waiting...", self.base.info.name);
        }
        self.limiter.until_ready().await;

        let req = request.build()?;
        debug!("Making request to: {}", req.url());
        self.base.client.execute(req).await
    }

    async fn send_get_json(&self, request: reqwest::RequestBuilder) -> Result<Json, FeedError> {
        let response = self.send(request).await?;

        let body = response.text().await?;
        let resp: Json = serde_json::from_str(&body)?;
        self.check_resp_errors(&resp)?;
        Ok(resp)
    }
}

#[async_trait]
impl Platform for ComickPlatform {
    async fn fetch_source(&self, slug: &str) -> Result<FeedSource, FeedError> {
        debug!(
            "Fetching info from {} for source_id: {slug}",
            self.base.info.name
        );

        let request = self
            .base
            .client
            .get(format!("{}/comic/{slug}", self.base.info.api_url));

        let resp = self.send_get_json(request).await?;
        let comic = self.get_comic_from_resp(&resp)?;

        let items_id = self.get_hid(comic)?;
        let name = self.get_title(comic)?;
        let description = self.get_description(comic)?;
        let source_url = self.get_source_url_from_id(slug);
        // We will assume image_url always exist for this platform until proven otherwise
        let image_url = Some(self.get_cover_url(comic)?);

        info!("Successfully fetched latest manga for source_id: {slug}");

        Ok(FeedSource {
            id: slug.to_string(),
            items_id,
            name,
            source_url,
            image_url,
            description,
        })
    }

    async fn fetch_latest(&self, hid: &str) -> Result<FeedItem, FeedError> {
        debug!(
            "Fetching latest from {} for source_id: {hid}",
            self.base.info.name
        );

        let request = self.base.client.get(format!(
            "{}/comic/{hid}/chapters?lang=en",
            self.base.info.api_url
        ));

        let resp = self.send_get_json(request).await?;

        let chapter = self.get_latest_chapter(&resp, hid)?;
        let title = self.get_chapter(chapter)?;
        let published = self.get_publish_at(chapter)?;

        Ok(FeedItem {
            id: hid.to_string(),
            title,
            published,
        })
    }

    fn get_id_from_source_url<'a>(&self, slug: &'a str) -> Result<&'a str, FeedError> {
        Ok(self.base.get_nth_path_from_url(slug, 1)?)
    }

    fn get_source_url_from_id(&self, slug: &str) -> String {
        format!("https://{}/comic/{}", self.base.info.api_domain, slug)
    }

    fn get_base(&self) -> &BasePlatform {
        &self.base
    }
}

impl PartialEq for ComickPlatform {
    fn eq(&self, other: &Self) -> bool {
        self.base.info.api_url == other.base.info.api_url
    }
}

impl Eq for ComickPlatform {}

impl Hash for ComickPlatform {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.base.info.api_url.hash(state);
    }
}
