//! Image generation for voice leaderboard.

use std::collections::HashMap;
use std::io::Cursor;
use std::time::Instant;

use ab_glyph::Font;
use ab_glyph::FontArc;
use ab_glyph::PxScale;
use anyhow::Result;
use image::DynamicImage;
use image::GenericImageView;
use image::Rgba;
use image::RgbaImage;
use image::imageops::FilterType;
use image::imageops::overlay;
use log::trace;

use crate::bot::commands::voice::LeaderboardEntry;

/// Dark gray background (Discord dark mode).
const BACKGROUND_COLOR: Rgba<u8> = Rgba([43, 45, 49, 255]);

/// Light text color.
const TEXT_COLOR: Rgba<u8> = Rgba([242, 243, 245, 255]);

/// Gold color for rank 1.
const GOLD_COLOR: Rgba<u8> = Rgba([250, 204, 21, 255]);

/// Silver color for rank 2.
const SILVER_COLOR: Rgba<u8> = Rgba([183, 184, 189, 255]);

/// Bronze color for rank 3.
const BRONZE_COLOR: Rgba<u8> = Rgba([235, 69, 158, 255]);

/// Gray placeholder color.
const PLACEHOLDER_COLOR: Rgba<u8> = Rgba([100, 100, 100, 255]);

/// Card background color.
const CARD_COLOR: Rgba<u8> = Rgba([49, 51, 56, 255]);

/// Progress bar color (low alpha).
const PROGRESS_COLOR: Rgba<u8> = Rgba([88, 101, 242, 60]);

/// Progress bar color for top 3.
const PROGRESS_TOP_COLOR: Rgba<u8> = Rgba([88, 101, 242, 100]);

/// Border color for cards.
const CARD_BORDER_COLOR: Rgba<u8> = Rgba([32, 34, 37, 255]);

/// Width of the generated image in pixels.
const IMAGE_WIDTH: u32 = 500;

/// Height per leaderboard entry.
const IMAGE_HEIGHT_PER_ENTRY: u32 = 64;

/// Padding around the image content.
const PADDING: u32 = 12;

/// Size of user avatars in pixels.
const AVATAR_SIZE: u32 = 40;

/// Font size for text rendering.
const FONT_SIZE: f32 = 22.0;

/// Vertical offset for text alignment.
const TEXT_VERTICAL_OFFSET: f32 = 6.0;

/// Cached glyph metrics for faster text rendering.
#[derive(Clone)]
struct GlyphCache {
    h_advance: f32,
}

/// Generates leaderboard images with user rankings.
pub struct LeaderboardImageGenerator {
    font: FontArc,
    pub http_client: reqwest::Client,
    glyph_cache: HashMap<char, GlyphCache>,
    avatar_cache: HashMap<String, RgbaImage>, // Cache processed circular avatars
}

impl LeaderboardImageGenerator {
    /// Creates a new image generator with embedded Roboto font.
    pub fn new() -> Result<Self> {
        // Load the Roboto font from embedded bytes
        let font_data = include_bytes!("../../../../assets/fonts/Roboto-Regular.ttf");
        let font = FontArc::try_from_slice(font_data)?;

        // Create HTTP client with connection pooling for faster avatar downloads
        let http_client = reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(10)
            .build()?;

        Ok(Self {
            font,
            http_client,
            glyph_cache: HashMap::with_capacity(256),
            avatar_cache: HashMap::new(),
        })
    }

    /// Checks if the avatar for the given URL is already cached.
    pub fn has_avatar(&self, url: &str) -> bool {
        self.avatar_cache.contains_key(url)
    }

    /// Downloads an avatar image from a URL.
    pub async fn download_avatar(&self, url: &str) -> Result<DynamicImage> {
        // Download avatar from the provided URL (could be WEBP or GIF)
        let response = self.http_client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to download avatar: {}",
                response.status()
            ));
        }

        let bytes = response.bytes().await?;
        let img = image::load_from_memory(&bytes)?;
        Ok(img)
    }

    /// Generates a leaderboard image with the given entries.
    pub async fn generate_leaderboard(&mut self, entries: &[LeaderboardEntry]) -> Result<Vec<u8>> {
        let total_start = Instant::now();

        // 1. Update avatar cache with new images provided in entries
        for entry in entries {
            if let Some(img) = &entry.avatar_image
                && !self.avatar_cache.contains_key(&entry.avatar_url)
            {
                let circular = self.make_circular(img, AVATAR_SIZE);
                self.avatar_cache.insert(entry.avatar_url.clone(), circular);
            }
        }

        // 2. Prepare data for renderer (clones necessary for 'static move closure)
        let font = self.font.clone();
        let glyph_cache = self.glyph_cache.clone();

        // Resolve images: prefer cached circular avatar
        let render_data: Vec<(LeaderboardEntry, Option<RgbaImage>)> = entries
            .iter()
            .map(|e| {
                let img = self.avatar_cache.get(&e.avatar_url).cloned();
                (e.clone(), img)
            })
            .collect();

        trace!("prepare_data {} ms", total_start.elapsed().as_millis());

        // 3. Spawn blocking task for CPU-intensive drawing
        let (bytes, new_glyph_cache) = tokio::task::spawn_blocking(move || {
            let mut renderer = Renderer { font, glyph_cache };
            let bytes = renderer.draw(render_data)?;
            Ok::<_, anyhow::Error>((bytes, renderer.glyph_cache))
        })
        .await??;

        // 4. Update glyph cache with any new metrics found during rendering
        self.glyph_cache = new_glyph_cache;

        trace!(
            "generate_leaderboard total {} ms",
            total_start.elapsed().as_millis()
        );

        Ok(bytes)
    }

    /// Crops an image to a circular shape.
    fn make_circular(&self, img: &DynamicImage, size: u32) -> RgbaImage {
        // Resize image to desired size
        let resized = img.resize_exact(size, size, FilterType::Lanczos3);

        // Create circular mask
        let mut circular = RgbaImage::new(size, size);
        let center = size as f32 / 2.0;
        let radius = size as f32 / 2.0;

        for (x, y, pixel) in resized.pixels() {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let distance = (dx * dx + dy * dy).sqrt();

            if distance <= radius {
                circular.put_pixel(x, y, pixel);
            } else {
                circular.put_pixel(x, y, Rgba([0, 0, 0, 0])); // Transparent
            }
        }

        circular
    }
}

/// Renderer for leaderboard images.
/// Handles the CPU-intensive drawing operations.
struct Renderer {
    font: FontArc,
    glyph_cache: HashMap<char, GlyphCache>,
}

impl Renderer {
    /// Draws the full leaderboard image.
    fn draw(&mut self, entries: Vec<(LeaderboardEntry, Option<RgbaImage>)>) -> Result<Vec<u8>> {
        let draw_start = Instant::now();
        let total_height = (entries.len() as u32 * IMAGE_HEIGHT_PER_ENTRY) + PADDING * 2;

        let mut img = RgbaImage::from_pixel(IMAGE_WIDTH, total_height, BACKGROUND_COLOR);

        // Get max duration for progress bar scaling
        let max_duration = entries.first().map(|(e, _)| e.duration_seconds).unwrap_or(0).max(1);

        for (idx, (entry, avatar)) in entries.iter().enumerate() {
            let y = PADDING + (idx as u32 * IMAGE_HEIGHT_PER_ENTRY);
            self.draw_entry(
                &mut img,
                y,
                entry.rank,
                avatar.as_ref(),
                &entry.display_name,
                entry.duration_seconds,
                max_duration,
            )?;
        }

        trace!(
            "draw_entries_blocking {} ms",
            draw_start.elapsed().as_millis()
        );

        let encode_start = Instant::now();
        let mut bytes: Vec<u8> = Vec::new();
        let mut cursor = Cursor::new(&mut bytes);
        // Use JPEG for smaller size and faster upload
        // JPEG doesn't support transparency, but our background is opaque anyway
        // except for the corners of circular avatars?
        // Wait, RgbaImage can have transparency. If we use JPEG, transparency becomes black.
        // Our background is dark blue, so transparency isn't critical for the main image,
        // but the circular avatars are overlayed on the dark background, so they should be fine.
        // The only issue is if the final image itself needs transparency.
        // The background color is opaque Rgba([26, 26, 46, 255]), so the whole image is opaque.
        // So JPEG is safe to use.

        // Convert to RgbImage for JPEG encoding (drops alpha channel)
        let rgb_img = image::DynamicImage::ImageRgba8(img).to_rgb8();
        rgb_img.write_to(&mut cursor, image::ImageFormat::Jpeg)?;

        trace!(
            "encode_jpeg_blocking {} ms",
            encode_start.elapsed().as_millis()
        );

        Ok(bytes)
    }

    /// Draws a single leaderboard entry at the given vertical position.
    fn draw_entry(
        &mut self,
        img: &mut RgbaImage,
        y: u32,
        rank: u32,
        avatar: Option<&RgbaImage>,
        display_name: &str,
        duration: i64,
        max_duration: i64,
    ) -> Result<()> {
        let card_x = PADDING;
        let card_y = y + 2; // Small gap between cards
        let card_w = IMAGE_WIDTH - (PADDING * 2);
        let card_h = IMAGE_HEIGHT_PER_ENTRY - 4;

        // Draw card background
        self.draw_rounded_rect(img, card_x, card_y, card_w, card_h, 8, CARD_COLOR);
        
        // Draw progress bar background
        let progress_width = ((duration as f32 / max_duration as f32) * card_w as f32) as u32;
        let progress_color = if rank <= 3 { PROGRESS_TOP_COLOR } else { PROGRESS_COLOR };
        if progress_width > 0 {
            self.draw_rounded_rect(img, card_x, card_y, progress_width, card_h, 8, progress_color);
        }

        // Draw card border
        self.draw_rounded_rect_outline(img, card_x, card_y, card_w, card_h, 8, CARD_BORDER_COLOR);

        // Calculate vertical center for this row
        let row_center_y = y + (IMAGE_HEIGHT_PER_ENTRY / 2);
        let text_baseline = row_center_y as f32 + TEXT_VERTICAL_OFFSET;

        // Determine rank color
        let rank_color = match rank {
            1 => GOLD_COLOR,
            2 => SILVER_COLOR,
            3 => BRONZE_COLOR,
            _ => TEXT_COLOR,
        };

        // Draw rank
        let rank_text = format!("#{}", rank);
        let rank_scale = PxScale::from(FONT_SIZE);
        let rank_x = card_x + 15;
        self.draw_text(
            img,
            &rank_text,
            rank_x as f32,
            text_baseline,
            rank_scale,
            rank_color,
        )?;

        // Draw avatar (circular) - centered vertically
        let avatar_x = rank_x + 45;
        let avatar_y = row_center_y.saturating_sub(AVATAR_SIZE / 2);

        if let Some(circular_avatar) = avatar {
            overlay(img, circular_avatar, avatar_x as i64, avatar_y as i64);
            // Optional: draw avatar border
            self.draw_circle_outline(img, avatar_x + AVATAR_SIZE / 2, avatar_y + AVATAR_SIZE / 2, AVATAR_SIZE / 2, CARD_BORDER_COLOR);
        } else {
            let circle_cx = avatar_x + AVATAR_SIZE / 2;
            let circle_cy = avatar_y + AVATAR_SIZE / 2;
            self.draw_circle_placeholder(img, circle_cx, circle_cy, AVATAR_SIZE / 2);
        }

        // Draw display name
        let name_x = avatar_x + AVATAR_SIZE + 12;
        self.draw_text(
            img,
            display_name,
            name_x as f32,
            text_baseline,
            rank_scale,
            TEXT_COLOR,
        )?;

        // Draw duration (right-aligned)
        let duration_text = format_duration(duration);
        let duration_width = self.calculate_text_width(&duration_text, rank_scale);
        let time_x = card_x + card_w - 15 - duration_width as u32;
        self.draw_text(
            img,
            &duration_text,
            time_x as f32,
            text_baseline,
            rank_scale,
            TEXT_COLOR,
        )?;

        Ok(())
    }

    /// Draws a rounded rectangle.
    fn draw_rounded_rect(&self, img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, radius: u32, color: Rgba<u8>) {
        for py in y..y + h {
            for px in x..x + w {
                if px >= img.width() || py >= img.height() { continue; }
                
                let dx = if px < x + radius { (x + radius) as i32 - px as i32 } 
                         else if px >= x + w - radius { px as i32 - (x + w - radius - 1) as i32 } 
                         else { 0 };
                let dy = if py < y + radius { (y + radius) as i32 - py as i32 } 
                         else if py >= y + h - radius { py as i32 - (y + h - radius - 1) as i32 } 
                         else { 0 };
                
                if dx > 0 && dy > 0 {
                    if (dx * dx + dy * dy) as f32 <= (radius * radius) as f32 {
                        self.blend_pixel(img, px, py, color);
                    }
                } else {
                    self.blend_pixel(img, px, py, color);
                }
            }
        }
    }

    /// Draws a rounded rectangle outline.
    fn draw_rounded_rect_outline(&self, img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, radius: u32, color: Rgba<u8>) {
        for py in y..y + h {
            for px in x..x + w {
                if px >= img.width() || py >= img.height() { continue; }
                
                let is_edge = px == x || px == x + w - 1 || py == y || py == y + h - 1;
                if !is_edge { continue; }

                let dx = if px < x + radius { (x + radius) as i32 - px as i32 } 
                         else if px >= x + w - radius { px as i32 - (x + w - radius - 1) as i32 } 
                         else { 0 };
                let dy = if py < y + radius { (y + radius) as i32 - py as i32 } 
                         else if py >= y + h - radius { py as i32 - (y + h - radius - 1) as i32 } 
                         else { 0 };
                
                if dx > 0 && dy > 0 {
                    let dist_sq = dx * dx + dy * dy;
                    if dist_sq as f32 <= (radius * radius) as f32 && dist_sq as f32 > ((radius-1) * (radius-1)) as f32 {
                        self.blend_pixel(img, px, py, color);
                    }
                } else {
                    self.blend_pixel(img, px, py, color);
                }
            }
        }
    }

    /// Draws a circle outline.
    fn draw_circle_outline(&self, img: &mut RgbaImage, cx: u32, cy: u32, radius: u32, color: Rgba<u8>) {
        let r_sq = (radius * radius) as f32;
        let r_inner_sq = ((radius.saturating_sub(1)) * (radius.saturating_sub(1))) as f32;
        for y in (cy.saturating_sub(radius))..=(cy + radius).min(img.height() - 1) {
            for x in (cx.saturating_sub(radius))..=(cx + radius).min(img.width() - 1) {
                let dx = x as f32 - cx as f32;
                let dy = y as f32 - cy as f32;
                let dist_sq = dx * dx + dy * dy;
                if dist_sq <= r_sq && dist_sq >= r_inner_sq {
                    self.blend_pixel(img, x, y, color);
                }
            }
        }
    }

    /// Blends a pixel with the existing content.
    fn blend_pixel(&self, img: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>) {
        let alpha = color[3] as f32 / 255.0;
        if alpha >= 1.0 {
            img.put_pixel(x, y, color);
        } else {
            let existing = img.get_pixel(x, y);
            let blended = Rgba([
                ((color[0] as f32 * alpha) + (existing[0] as f32 * (1.0 - alpha))) as u8,
                ((color[1] as f32 * alpha) + (existing[1] as f32 * (1.0 - alpha))) as u8,
                ((color[2] as f32 * alpha) + (existing[2] as f32 * (1.0 - alpha))) as u8,
                255,
            ]);
            img.put_pixel(x, y, blended);
        }
    }

    /// Calculates the width of text at the given scale using cached glyph metrics.
    fn calculate_text_width(&mut self, text: &str, scale: PxScale) -> f32 {
        let mut width = 0.0;
        let scale_factor = scale.x / self.font.height_unscaled();

        for c in text.chars() {
            let cache = self.glyph_cache.entry(c).or_insert_with(|| {
                let glyph_id = self.font.glyph_id(c);
                let h_advance = self.font.h_advance_unscaled(glyph_id);
                GlyphCache { h_advance }
            });
            width += cache.h_advance * scale_factor;
        }
        width
    }

    /// Draws text onto the image at the given position using cached glyph metrics.
    fn draw_text(
        &mut self,
        img: &mut RgbaImage,
        text: &str,
        mut x: f32,
        y: f32,
        scale: PxScale,
        color: Rgba<u8>,
    ) -> Result<()> {
        use ab_glyph::Glyph;

        let scale_factor = scale.x / self.font.height_unscaled();

        for c in text.chars() {
            // Use cached glyph metrics
            let cache = self.glyph_cache.entry(c).or_insert_with(|| {
                let glyph_id = self.font.glyph_id(c);
                let h_advance = self.font.h_advance_unscaled(glyph_id);
                GlyphCache { h_advance }
            });

            let glyph_id = self.font.glyph_id(c);
            let glyph: Glyph = glyph_id.with_scale_and_position(scale.x, ab_glyph::point(x, y));

            if let Some(outlined) = self.font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, gv| {
                    let px = gx as i32 + bounds.min.x as i32;
                    let py = gy as i32 + bounds.min.y as i32;

                    if px >= 0 && px < img.width() as i32 && py >= 0 && py < img.height() as i32 {
                        let pixel = img.get_pixel(px as u32, py as u32);
                        let blended = Rgba([
                            ((color[0] as f32 * gv) + (pixel[0] as f32 * (1.0 - gv))) as u8,
                            ((color[1] as f32 * gv) + (pixel[1] as f32 * (1.0 - gv))) as u8,
                            ((color[2] as f32 * gv) + (pixel[2] as f32 * (1.0 - gv))) as u8,
                            255,
                        ]);

                        img.put_pixel(px as u32, py as u32, blended);
                    }
                });
            }

            // Advance x position using cached advance
            x += cache.h_advance * scale_factor;
        }

        Ok(())
    }

    /// Draws a circular placeholder avatar.
    fn draw_circle_placeholder(&self, img: &mut RgbaImage, cx: u32, cy: u32, radius: u32) {
        for y in (cy.saturating_sub(radius))..=(cy + radius).min(img.height() - 1) {
            for x in (cx.saturating_sub(radius))..=(cx + radius).min(img.width() - 1) {
                let dx = x as f32 - cx as f32;
                let dy = y as f32 - cy as f32;
                let distance = (dx * dx + dy * dy).sqrt();

                if distance <= radius as f32 {
                    img.put_pixel(x, y, PLACEHOLDER_COLOR);
                }
            }
        }
    }
}

/// Formats a duration in seconds into a human-readable string.
fn format_duration(seconds: i64) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(3660), "1h 1m");
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90000), "1d 1h");
    }
}
