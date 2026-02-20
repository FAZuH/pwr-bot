//! Image generation for voice leaderboard.

use std::collections::HashMap;
use std::io::Cursor;
use std::time::Instant;

use anyhow::Result;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::DynamicImage;
use image::imageops::FilterType;
use log::trace;
use minijinja::Environment;
use minijinja::context;
use serde::Serialize;

use crate::bot::commands::voice::leaderboard::image_builder::LeaderboardEntry;
use crate::bot::utils::format_duration;

const IMAGE_WIDTH: u32 = 500;
const IMAGE_HEIGHT_PER_ENTRY: u32 = 64;
const PADDING: u32 = 12;
const AVATAR_SIZE: u32 = 40;

const GOLD_COLOR: &str = "#FACC15";
const SILVER_COLOR: &str = "#B7B8BD";
const BRONZE_COLOR: &str = "#EB459E";
const TEXT_COLOR: &str = "#F2F3F5";
const PROGRESS_COLOR: &str = "rgba(88, 101, 242, 0.235)";
const PROGRESS_TOP_COLOR: &str = "rgba(88, 101, 242, 0.392)";

/// Defines the exact data structure expected by the Minijinja SVG template.
#[derive(Serialize)]
struct TemplateEntry {
    rank: u32,
    rank_color: &'static str,
    name: String,
    duration: String,

    // Layout metrics calculated by Rust
    card_y: u32,
    progress_width: u32,
    progress_color: &'static str,
    text_baseline: f32,
    avatar_y: u32,
    avatar_cx: u32,
    avatar_cy: u32,

    // Processed data
    avatar_b64: Option<String>,
}

pub struct LeaderboardImageGenerator {
    pub http_client: reqwest::Client,
    avatar_cache: HashMap<String, String>,
    jinja_env: Environment<'static>,
}

impl LeaderboardImageGenerator {
    pub fn new() -> Self {
        let http_client = reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(10)
            .build()
            .expect("Failed to build HTTP client");

        // Initialize template engine and load the template
        let mut jinja_env = Environment::new();
        let template_str = include_str!("../../../../../assets/leaderboard.svg");
        jinja_env.add_template("leaderboard", template_str).unwrap();

        Self {
            http_client,
            avatar_cache: HashMap::new(),
            jinja_env,
        }
    }

    pub fn has_avatar(&self, url: &str) -> bool {
        self.avatar_cache.contains_key(url)
    }

    pub async fn download_avatar(&self, url: &str) -> Result<String> {
        let response = self.http_client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download avatar: {}",
                response.status()
            ));
        }
        let bytes = response.bytes().await?;
        let img = image::load_from_memory(&bytes)?;
        Ok(self.process_avatar_to_b64(&img))
    }

    fn process_avatar_to_b64(&self, img: &DynamicImage) -> String {
        let resized = img.resize_exact(AVATAR_SIZE, AVATAR_SIZE, FilterType::Lanczos3);
        let mut cursor = Cursor::new(Vec::new());
        resized
            .write_to(&mut cursor, image::ImageFormat::Png)
            .unwrap();
        BASE64.encode(cursor.into_inner())
    }

    pub async fn generate_leaderboard(&mut self, entries: &[LeaderboardEntry]) -> Result<Vec<u8>> {
        let total_start = Instant::now();

        // 1. Ensure all avatars are cached
        for entry in entries {
            if let Some(img) = &entry.avatar_image
                && !self.avatar_cache.contains_key(&entry.avatar_url) {
                    let b64 = self.process_avatar_to_b64(img);
                    self.avatar_cache.insert(entry.avatar_url.clone(), b64);
                }
        }

        // 2. Pre-calculate metrics to keep the template logic-less
        let total_height = (entries.len() as u32 * IMAGE_HEIGHT_PER_ENTRY) + PADDING * 2;
        let max_duration = entries
            .first()
            .map(|e| e.duration_seconds)
            .unwrap_or(0)
            .max(1);
        let card_w = IMAGE_WIDTH - (PADDING * 2);
        let time_x = PADDING + card_w - 15;

        // 3. Map entries to the template structure
        let template_entries: Vec<TemplateEntry> = entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let y = PADDING + (idx as u32 * IMAGE_HEIGHT_PER_ENTRY);
                let row_center_y = y + (IMAGE_HEIGHT_PER_ENTRY / 2);
                let avatar_y = row_center_y.saturating_sub(AVATAR_SIZE / 2);

                let rank_color = match entry.rank {
                    1 => GOLD_COLOR,
                    2 => SILVER_COLOR,
                    3 => BRONZE_COLOR,
                    _ => TEXT_COLOR,
                };

                let progress_width =
                    ((entry.duration_seconds as f32 / max_duration as f32) * card_w as f32) as u32;
                let progress_color = if entry.rank <= 3 {
                    PROGRESS_TOP_COLOR
                } else {
                    PROGRESS_COLOR
                };

                TemplateEntry {
                    rank: entry.rank,
                    rank_color,
                    name: entry.display_name.clone(), // Minijinja auto-escapes HTML/XML by default
                    duration: format_duration(entry.duration_seconds),
                    card_y: y + 2,
                    progress_width,
                    progress_color,
                    text_baseline: row_center_y as f32 + 6.0,
                    avatar_y,
                    avatar_cx: 72 + (AVATAR_SIZE / 2),
                    avatar_cy: avatar_y + (AVATAR_SIZE / 2),
                    avatar_b64: self.avatar_cache.get(&entry.avatar_url).cloned(),
                }
            })
            .collect();

        // 4. Render the template
        let template = self.jinja_env.get_template("leaderboard")?;
        let svg = template.render(context! {
            image_width => IMAGE_WIDTH,
            total_height => total_height,
            card_w => card_w,
            card_h => IMAGE_HEIGHT_PER_ENTRY - 4,
            time_x => time_x,
            entries => template_entries,
        })?;

        trace!(
            "generate_leaderboard total {} ms",
            total_start.elapsed().as_millis()
        );

        let png = Self::svg_to_png(&svg, IMAGE_WIDTH, total_height)?;
        Ok(png)
    }

    fn svg_to_png(svg: &str, width: u32, height: u32) -> Result<Vec<u8>> {
        let mut fontdb = resvg::usvg::fontdb::Database::new();
        fontdb.load_font_data(
            include_bytes!("../../../../../assets/fonts/Roboto-Regular.ttf").to_vec(),
        );

        let options = resvg::usvg::Options {
            fontdb: std::sync::Arc::new(fontdb),
            ..Default::default()
        };

        let tree = resvg::usvg::Tree::from_str(svg, &options)?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
            .ok_or_else(|| anyhow::anyhow!("Failed to create pixmap"))?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::default(),
            &mut pixmap.as_mut(),
        );
        Ok(pixmap.encode_png()?)
    }
}
