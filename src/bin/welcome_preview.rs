use std::path::Path;

use anyhow::Result;
use image::RgbaImage;
use pwr_bot::bot::commands::welcome::image_generator::WelcomeCardData;
use pwr_bot::bot::commands::welcome::image_generator::WelcomeImageGenerator;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Generating preview images for welcome templates...");

    let generator = WelcomeImageGenerator::new();
    let mut images = Vec::new();

    let template_width = 800;
    let template_height = 300;
    let padding_x = 40;
    let padding_y = 40;
    let cols = 3;
    let rows = 4;

    let total_width = cols * template_width + (cols + 1) * padding_x;
    let label_height = 40;
    let total_height = rows * (template_height + label_height) + (rows + 1) * padding_y;

    let mut combined_image =
        RgbaImage::from_pixel(total_width, total_height, image::Rgba([30, 31, 34, 255])); // Discord dark theme background

    for i in 1..=12 {
        println!("Generating template {}...", i);

        let data = WelcomeCardData {
            template_id: i.to_string(),
            username: "FAZuH".to_string(),
            user_tag: "@fazuh".to_string(),
            avatar_url: "https://cdn.discordapp.com/avatars/257428751560867840/9afb22958d5bbb3e91fb077ca546c821.png".to_string(),
            avatar_b64: None,
            server_name: format!("Template #{}", i),
            member_count: "100".to_string(),
            member_number: "#100".to_string(),
            primary_color: "#5865F2".to_string(),
            welcome_message: format!("Preview for Template {}", i),
        };

        let png_bytes = generator.generate_card(data).await?;
        let img = image::load_from_memory(&png_bytes)?;

        images.push(img);
    }

    println!("Combining images into grid...");

    for (idx, img) in images.iter().enumerate() {
        let col = (idx as u32) % cols;
        let row = (idx as u32) / cols;

        let x = padding_x + col * (template_width + padding_x);
        let y = padding_y + row * (template_height + label_height + padding_y);

        // Render template label
        let label_svg = format!(
            r#"<svg width="{}" height="{}" xmlns="http://www.w3.org/2000/svg">
                <text x="50%" y="28" font-family="Roboto" font-size="28" font-weight="bold" fill="white" text-anchor="middle">Template {}</text>
            </svg>"#,
            template_width,
            label_height,
            idx + 1
        );
        if let Ok(label_png) =
            WelcomeImageGenerator::svg_to_png(&label_svg, template_width, label_height)
        {
            if let Ok(label_img) = image::load_from_memory(&label_png) {
                image::imageops::overlay(&mut combined_image, &label_img, x.into(), y.into());
            }
        }

        image::imageops::overlay(
            &mut combined_image,
            img,
            x.into(),
            (y + label_height).into(),
        );
    }

    let out_path = Path::new("docs/welcome_templates_preview.png");
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    combined_image.save(out_path)?;
    println!("Successfully saved preview to {:?}", out_path);

    Ok(())
}
