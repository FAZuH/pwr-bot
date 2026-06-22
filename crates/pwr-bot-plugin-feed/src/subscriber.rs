use deadpool_postgres::Pool;
use pwr_bot_plugin_util::query_json;
use pwr_bot_sdk::*;
use serenity::all::*;

fn extract_feed_id(payload: &serde_json::Value) -> Option<i64> {
    payload
        .get("feed")
        .and_then(|f| f.get("id"))
        .and_then(|v| v.as_i64())
}

fn build_message(payload: &serde_json::Value) -> ResponsePayload {
    let feed_name = payload
        .get("feed")
        .and_then(|f| f.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown feed");
    let feed_description = payload
        .get("feed")
        .and_then(|f| f.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let platform_logo_url = payload
        .get("feed")
        .and_then(|f| f.get("logo_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let cover_url = payload
        .get("feed")
        .and_then(|f| f.get("cover_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let source_url = payload
        .get("feed")
        .and_then(|f| f.get("source_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let item_name = payload
        .get("item_name")
        .and_then(|v| v.as_str())
        .unwrap_or("item");
    let new_title = payload
        .get("latest")
        .and_then(|l| l.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let new_published = payload
        .get("latest")
        .and_then(|l| l.get("published"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let old_title = payload
        .get("old_title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let old_published = payload
        .get("old_published")
        .and_then(|v| v.as_i64())
        .filter(|v| *v != 0);
    let copyright_notice = payload
        .get("copyright_notice")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let feed_desc = feed_description
        .trim()
        .replace('\n', "\n> ")
        .replace("<br>", "");

    let mut text_main = format!("### {feed_name}\n\n> {feed_desc}\n\n");

    if let (Some(old_t), Some(old_ts)) = (old_title, old_published) {
        text_main.push_str(&format!(
            "**Old {item_name}**: {old_t}\nPublished on <t:{old_ts}>\n\n"
        ));
    }

    text_main.push_str(&format!(
        "**New {item_name}**: {new_title}\nPublished on <t:{new_published}>\n\n"
    ));

    if !source_url.is_empty() {
        text_main.push_str(&format!("**[Open in browser \u{2197}]({source_url})**"));
    }

    let mut container_components: Vec<CreateContainerComponent> = Vec::new();

    let section = CreateSection::new(
        vec![CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
            text_main,
        ))],
        if !platform_logo_url.is_empty() {
            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(CreateUnfurledMediaItem::new(
                platform_logo_url,
            )))
        } else {
            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(CreateUnfurledMediaItem::new(
                "",
            )))
        },
    );
    container_components.push(CreateContainerComponent::Section(section));

    container_components.push(CreateContainerComponent::Separator(CreateSeparator::new(
        false,
    )));

    if !cover_url.is_empty() {
        container_components.push(CreateContainerComponent::MediaGallery(
            CreateMediaGallery::new(vec![CreateMediaGalleryItem::new(
                CreateUnfurledMediaItem::new(cover_url),
            )]),
        ));
    }

    if !copyright_notice.is_empty() {
        container_components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(format!("-# {copyright_notice}")),
        ));
    }

    let container = CreateContainer::new(container_components);

    let create_message = CreateMessage::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(vec![CreateComponent::Container(container)]);

    ResponsePayload::from_serializable(&create_message)
        .unwrap_or_else(|_| ResponsePayload::text(format!("New {item_name}: {new_title}")))
}

async fn query_subscribers(pool: &Pool, feed_id: i64, sub_type: &str) -> Vec<serde_json::Value> {
    let result = query_json(
        pool,
        "SELECT s.id, s.target_id, s.type \
         FROM subscribers s \
         JOIN feed_subscriptions fs ON fs.subscriber_id = s.id \
         WHERE fs.feed_id = $1 AND s.type = $2",
        &[&feed_id, &sub_type],
    )
    .await
    .unwrap_or(serde_json::Value::Array(vec![]));

    result.as_array().cloned().unwrap_or_default()
}

pub async fn handle_feed_update(
    pool: &Pool,
    host: &PluginHost,
    payload: serde_json::Value,
) -> Result<(), String> {
    let feed_id = match extract_feed_id(&payload) {
        Some(id) => id,
        None => return Err("FeedUpdateEvent missing feed.id".to_string()),
    };

    let response = build_message(&payload);
    let json_str = serde_json::to_string(&response).map_err(|e| e.to_string())?;

    let guild_subs = query_subscribers(pool, feed_id, "guild").await;
    for sub in &guild_subs {
        let target_id = match sub.get("target_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };
        let guild_id: u64 = match target_id.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };
        let _ = unsafe { host.send_channel_message(guild_id, &json_str) };
    }

    let dm_subs = query_subscribers(pool, feed_id, "dm").await;
    for sub in &dm_subs {
        let target_id = match sub.get("target_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };
        let user_id: u64 = match target_id.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };
        let _ = unsafe { host.send_dm(user_id, &json_str) };
    }

    Ok(())
}
