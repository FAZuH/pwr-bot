use std::io::Cursor;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::DynamicImage;
use image::imageops::FilterType;

pub fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        if minutes > 0 {
            format!("{hours}h {minutes}m")
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

pub async fn download_avatar(
    http_client: &wreq::Client,
    url: &str,
    size: u32,
) -> anyhow::Result<String> {
    let response = http_client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "failed to download avatar: {}",
            response.status()
        ));
    }
    let bytes = response.bytes().await?;
    let image = image::load_from_memory(&bytes)?;
    Ok(avatar_to_b64(&image, size))
}

pub fn avatar_to_b64(image: &DynamicImage, size: u32) -> String {
    let resized = image.resize_exact(size, size, FilterType::Lanczos3);
    let mut cursor = Cursor::new(Vec::new());
    resized
        .write_to(&mut cursor, image::ImageFormat::Png)
        .expect("PNG encoding succeeds");
    BASE64.encode(cursor.into_inner())
}
