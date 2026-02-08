use std::io::Cursor;

use ab_glyph::{Font, FontArc, PxScale};
use anyhow::Result;
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use image::imageops::{overlay, FilterType};

const BACKGROUND_COLOR: Rgba<u8> = Rgba([26, 26, 46, 255]); // Dark blue-purple #1a1a2e
const TEXT_COLOR: Rgba<u8> = Rgba([224, 224, 224, 255]); // Light gray #e0e0e0
const GOLD_COLOR: Rgba<u8> = Rgba([255, 215, 0, 255]); // Gold #ffd700
const SILVER_COLOR: Rgba<u8> = Rgba([192, 192, 192, 255]); // Silver #c0c0c0
const BRONZE_COLOR: Rgba<u8> = Rgba([205, 127, 50, 255]); // Bronze #cd7f32
const PLACEHOLDER_COLOR: Rgba<u8> = Rgba([100, 100, 100, 255]); // Gray placeholder

const IMAGE_WIDTH: u32 = 400; // Half width as requested
const IMAGE_HEIGHT_PER_ENTRY: u32 = 50;
const PADDING: u32 = 15;
const AVATAR_SIZE: u32 = 32; // Smaller avatar to match font height
const FONT_SIZE: f32 = 20.0;
const TEXT_VERTICAL_OFFSET: f32 = 6.0; // Adjust text baseline

pub struct LeaderboardImageGenerator {
    font: FontArc,
    http_client: reqwest::Client,
}

impl LeaderboardImageGenerator {
    pub fn new() -> Result<Self> {
        // Load the Roboto font from embedded bytes
        let font_data = include_bytes!("../../../../assets/fonts/Roboto-Regular.ttf");
        let font = FontArc::try_from_slice(font_data)?;
        
        Ok(Self {
            font,
            http_client: reqwest::Client::new(),
        })
    }

    pub async fn generate_leaderboard(
        &self,
        entries: &[(u32, u64, String, Option<String>, i64)], // (rank, user_id, display_name, avatar_url, duration_seconds)
    ) -> Result<Vec<u8>> {
        let total_height = (entries.len() as u32 * IMAGE_HEIGHT_PER_ENTRY) + PADDING * 2;
        
        // Create base image with dark background
        let mut img = RgbaImage::from_pixel(IMAGE_WIDTH, total_height, BACKGROUND_COLOR);
        
        // Draw entries
        for (idx, (rank, user_id, display_name, avatar_url, duration)) in entries.iter().enumerate() {
            let y = PADDING + (idx as u32 * IMAGE_HEIGHT_PER_ENTRY);
            
            // Download and process avatar
            let avatar = match avatar_url {
                Some(url) => self.download_avatar_from_url(url).await.ok(),
                None => self.download_default_avatar(*user_id).await.ok(),
            };
            
            self.draw_entry(&mut img, y, *rank, avatar.as_ref(), display_name, *duration)?;
        }
        
        // Encode to PNG
        let mut bytes: Vec<u8> = Vec::new();
        let mut cursor = Cursor::new(&mut bytes);
        img.write_to(&mut cursor, image::ImageFormat::Png)?;
        
        Ok(bytes)
    }

    fn draw_entry(
        &self,
        img: &mut RgbaImage,
        y: u32,
        rank: u32,
        avatar: Option<&DynamicImage>,
        display_name: &str,
        duration: i64,
    ) -> Result<()> {
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
        
        // Draw rank (centered vertically with other elements)
        let rank_text = format!("#{} •", rank);
        let rank_scale = PxScale::from(FONT_SIZE);
        self.draw_text(img, &rank_text, PADDING as f32, text_baseline, rank_scale, rank_color)?;
        
        // Calculate rank text width to position avatar
        let rank_text_width = self.calculate_text_width(&rank_text, rank_scale);
        
        // Draw avatar (circular) - centered vertically
        let avatar_x = PADDING + rank_text_width as u32 + 5;
        let avatar_y = row_center_y.saturating_sub(AVATAR_SIZE / 2);
        
        if let Some(avatar_img) = avatar {
            let circular_avatar = self.make_circular(avatar_img, AVATAR_SIZE);
            overlay(img, &circular_avatar, avatar_x as i64, avatar_y as i64);
        } else {
            // Draw placeholder circle - centered vertically
            let circle_cx = avatar_x + AVATAR_SIZE / 2;
            let circle_cy = avatar_y + AVATAR_SIZE / 2;
            self.draw_circle_placeholder(img, circle_cx, circle_cy, AVATAR_SIZE / 2);
        }
        
        // Draw display name (centered vertically)
        let name_x = avatar_x + AVATAR_SIZE + 10;
        self.draw_text(
            img,
            display_name,
            name_x as f32,
            text_baseline,
            rank_scale,
            TEXT_COLOR,
        )?;
        
        // Draw duration (right-aligned, centered vertically)
        let duration_text = format_duration(duration);
        let duration_width = self.calculate_text_width(&duration_text, rank_scale);
        let time_x = IMAGE_WIDTH - PADDING - duration_width as u32;
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

    fn calculate_text_width(&self, text: &str, scale: PxScale) -> f32 {
        let mut width = 0.0;
        for c in text.chars() {
            let glyph_id = self.font.glyph_id(c);
            let h_advance = self.font.h_advance_unscaled(glyph_id);
            let scale_factor = scale.x / self.font.height_unscaled();
            width += h_advance * scale_factor;
        }
        width
    }

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

    async fn download_avatar_from_url(&self, url: &str) -> Result<DynamicImage> {
        // Download avatar from the provided URL (could be WEBP or GIF)
        let response = self.http_client.get(url).send().await?;
        
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to download avatar: {}", response.status()));
        }
        
        let bytes = response.bytes().await?;
        let img = image::load_from_memory(&bytes)?;
        Ok(img)
    }

    async fn download_default_avatar(&self, user_id: u64) -> Result<DynamicImage> {
        // Discord uses a discriminator-based default avatar if no custom avatar is set
        let discriminator = user_id % 5;
        let url = format!(
            "https://cdn.discordapp.com/embed/avatars/{}.png",
            discriminator
        );
        
        let response = self.http_client.get(&url).send().await?;
        
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Failed to download default avatar: {}", response.status()));
        }
        
        let bytes = response.bytes().await?;
        let img = image::load_from_memory(&bytes)?;
        Ok(img)
    }

    fn draw_text(
        &self,
        img: &mut RgbaImage,
        text: &str,
        mut x: f32,
        y: f32,
        scale: PxScale,
        color: Rgba<u8>,
    ) -> Result<()> {
        use ab_glyph::Glyph;
        
        for c in text.chars() {
            let glyph_id = self.font.glyph_id(c);
            let h_advance_unscaled = self.font.h_advance_unscaled(glyph_id);
            
            let glyph: Glyph = glyph_id
                .with_scale_and_position(scale.x, ab_glyph::point(x, y));
            
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
            
            // Advance x position for next character using unscaled advance
            let scale_factor = scale.x / self.font.height_unscaled();
            x += h_advance_unscaled * scale_factor;
        }
        
        Ok(())
    }
}

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
