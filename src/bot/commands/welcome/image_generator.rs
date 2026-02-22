//! Image generation for welcome cards.

use std::io::Cursor;

use anyhow::Result;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::DynamicImage;
use image::imageops::FilterType;
use minijinja::Environment;
use serde::Serialize;

const AVATAR_SIZE: u32 = 128; // Adjust based on templates, using a larger one is safe

/// Defines the exact data structure expected by the Minijinja SVG template.
#[derive(Serialize)]
pub struct WelcomeCardData {
    pub template_id: String,
    pub username: String,
    pub user_tag: String,
    pub avatar_url: String,
    pub avatar_b64: Option<String>, // Generated internally or provided
    pub server_name: String,
    pub member_count: String,
    pub member_number: String,
    pub primary_color: String,
    pub welcome_message: String,
}

pub struct WelcomeImageGenerator {
    pub http_client: reqwest::Client,
    jinja_env: Environment<'static>,
}

impl WelcomeImageGenerator {
    pub fn new() -> Self {
        let http_client = reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(10)
            .build()
            .expect("Failed to build HTTP client");

        let mut jinja_env = Environment::new();

        // Add all templates
        jinja_env
            .add_template("1", include_str!("../../../../assets/welcome/1.svg"))
            .unwrap();
        jinja_env
            .add_template("2", include_str!("../../../../assets/welcome/2.svg"))
            .unwrap();
        jinja_env
            .add_template("3", include_str!("../../../../assets/welcome/3.svg"))
            .unwrap();
        jinja_env
            .add_template("4", include_str!("../../../../assets/welcome/4.svg"))
            .unwrap();
        jinja_env
            .add_template("5", include_str!("../../../../assets/welcome/5.svg"))
            .unwrap();
        jinja_env
            .add_template("6", include_str!("../../../../assets/welcome/6.svg"))
            .unwrap();
        jinja_env
            .add_template("7", include_str!("../../../../assets/welcome/7.svg"))
            .unwrap();
        jinja_env
            .add_template("8", include_str!("../../../../assets/welcome/8.svg"))
            .unwrap();
        jinja_env
            .add_template("9", include_str!("../../../../assets/welcome/9.svg"))
            .unwrap();
        jinja_env
            .add_template("10", include_str!("../../../../assets/welcome/10.svg"))
            .unwrap();
        jinja_env
            .add_template("11", include_str!("../../../../assets/welcome/11.svg"))
            .unwrap();
        jinja_env
            .add_template("12", include_str!("../../../../assets/welcome/12.svg"))
            .unwrap();

        Self {
            http_client,
            jinja_env,
        }
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

    pub async fn generate_card(&self, mut data: WelcomeCardData) -> Result<Vec<u8>> {
        // Placeholder: 1x1 gray pixel as fallback avatar
        const PLACEHOLDER_AVATAR: &str = "iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAABHNCSVQICAgIfAhkiAAAAAlwSFlzAAAAdgAAAHYBTnsmCAAAABl0RVh0U29mdHdhcmUAd3d3Lmlua3NjYXBlLm9yZ5vuPBoAAADASURBVDiNY2AYBYMBMDIw/P/PyMDwn5GBgYGRgeE/I8N/BkYGBgZGRgYGRob/DIz//zMw/mdk+P+fkfE/AyPDfwbG//8ZGP//Z2D8z8D4n4Hh/38Ghv8MDAwMjIwMDAz//2dg/G9k+P+/gRHJAMD//z8DI7YBZ3h4FIiBgfH/PyMD4z8jw38GBoamHBgYGBgYGBgYGBj+/2NgYGBgYGBgYGBgYPjPYGBgYGBgYGBgYGBg+A8jI8N/BkYGBkaGBgZGhgYGhlIMDAwMjIwMDIwMDIwMDIyMjIyMjAwMjIwMDIwMDIyM/wwMjAwMDIzMDAzM//9nYGBgYICBgfF/BgYGBkZGBgZGRgYGRob/DIwMDIyMDAwMjAwMjIz8GRgYGBgZGRkYGRkZGRkZGf4zMDAwMDIyMjAyMjIyMjIyMv9nYGBgYICBgYGBgYHhPwPDfwbG/wwM//9nYPjPoAMPABw7JKxFaM0lAAAAAElFTkSuQmCC";

        if data.avatar_b64.is_none() && !data.avatar_url.is_empty() {
            data.avatar_b64 = Some(
                self.download_avatar(&data.avatar_url)
                    .await
                    .unwrap_or_else(|_| PLACEHOLDER_AVATAR.to_string()),
            );
        } else if data.avatar_b64.is_none() {
            data.avatar_b64 = Some(PLACEHOLDER_AVATAR.to_string());
        }

        let template = self
            .jinja_env
            .get_template(&data.template_id)
            .unwrap_or_else(|_| self.jinja_env.get_template("1").unwrap());
        let svg = template.render(&data)?;

        let width = 800; // Will be determined by svg
        let height = 300;

        let png = Self::svg_to_png(&svg, width, height)?;
        Ok(png)
    }

    pub fn svg_to_png(svg: &str, _width: u32, _height: u32) -> Result<Vec<u8>> {
        let mut fontdb = resvg::usvg::fontdb::Database::new();
        fontdb
            .load_font_data(include_bytes!("../../../../assets/fonts/Roboto-Regular.ttf").to_vec());

        // Map all generic font families to Roboto to ensure text always renders
        fontdb.set_sans_serif_family("Roboto");
        fontdb.set_serif_family("Roboto");
        fontdb.set_cursive_family("Roboto");
        fontdb.set_fantasy_family("Roboto");
        fontdb.set_monospace_family("Roboto");

        let options = resvg::usvg::Options {
            fontdb: std::sync::Arc::new(fontdb),
            ..Default::default()
        };

        let tree = resvg::usvg::Tree::from_str(svg, &options)?;

        // Use intrinsic size
        let svg_width = tree.size().width() as u32;
        let svg_height = tree.size().height() as u32;

        let mut pixmap = resvg::tiny_skia::Pixmap::new(svg_width, svg_height)
            .ok_or_else(|| anyhow::anyhow!("Failed to create pixmap"))?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::default(),
            &mut pixmap.as_mut(),
        );
        Ok(pixmap.encode_png()?)
    }
}
