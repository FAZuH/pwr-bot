use pwr_bot_sdk::*;

pub struct FeedPlugin;

#[async_trait::async_trait]
impl BotPlugin for FeedPlugin {
    fn name(&self) -> &'static str {
        "feed"
    }

    fn description(&self) -> &'static str {
        "Feed subscription management"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn commands(&self) -> Vec<CommandSpec> {
        vec![CommandSpec {
            name: "feed".into(),
            description: "Manage feed subscriptions".into(),
            args: vec![],
        }]
    }

    async fn invoke(
        &self,
        command: &str,
        _args: serde_json::Value,
        host: &PluginHost,
    ) -> Result<ResponsePayload, String> {
        match command {
            "feed" | "feed list" => self.cmd_list(host).await,
            _ => Err(format!("Unknown command: {command}")),
        }
    }
}

impl FeedPlugin {
    async fn cmd_list(&self, host: &PluginHost) -> Result<ResponsePayload, String> {
        unsafe { host.defer().map_err(|e| e.to_string())? };

        let _guild_id = unsafe { host.guild_id() };
        let author_id = unsafe { host.author_id() };

        // Get subscriber for DM (default for listing)
        let result = unsafe {
            host.query_db(
                "SELECT s.id, s.type_, s.target_id \
                 FROM subscribers s \
                 WHERE s.target_id = $1 AND s.type_ = 'dm'",
                &[DbValue::I64(author_id as i64)],
            )
            .map_err(|e| format!("DB query failed: {e}"))?
        };

        let rows = match result {
            serde_json::Value::Array(arr) => arr,
            _ => vec![],
        };

        if rows.is_empty() {
            return Ok(ResponsePayload {
                content: Some(
                    "You have no subscriptions. Use `/feed subscribe` to add some!".into(),
                ),
                ephemeral: false,
                components_json: None,
                embed_json: None,
            });
        }

        let subscriber_id = rows[0].get("id").and_then(|v| v.as_i64()).unwrap_or(0);

        // Get subscriptions with feed details
        let feed_result = unsafe {
            host.query_db(
                "SELECT f.id, f.name, f.description, f.platform_id, f.source_id, \
                        f.items_id, f.source_url, f.cover_url, f.tags, \
                        fi.description as item_desc, fi.published as item_published \
                 FROM feed_subscriptions fs \
                 JOIN feeds f ON f.id = fs.feed_id \
                 LEFT JOIN feed_items fi ON fi.id = f.items_id \
                 WHERE fs.subscriber_id = $1 \
                 ORDER BY f.name",
                &[DbValue::I64(subscriber_id)],
            )
            .map_err(|e| format!("DB query failed: {e}"))?
        };

        let feeds = match feed_result {
            serde_json::Value::Array(arr) => arr,
            _ => vec![],
        };

        let mut lines: Vec<String> = Vec::new();
        lines.push("## Your Subscriptions".to_string());

        for (i, feed) in feeds.iter().enumerate() {
            let name = feed
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let source_url = feed
                .get("source_url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let tags = feed.get("tags").and_then(|v| v.as_str()).unwrap_or("");
            let platform = feed
                .get("platform_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let item_desc = feed
                .get("item_desc")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());

            let line = match item_desc {
                Some(latest) => {
                    format!(
                        "{}. **[{name}](<{source_url}>)** ({platform})\n   └ Latest: {latest}",
                        i + 1
                    )
                }
                None => {
                    format!("{}. **[{name}](<{source_url}>)** ({platform})", i + 1)
                }
            };
            lines.push(line);

            if !tags.is_empty() {
                lines.push(format!("   └ Tags: {tags}"));
            }
        }

        Ok(ResponsePayload {
            content: Some(lines.join("\n")),
            ephemeral: false,
            components_json: None,
            embed_json: None,
        })
    }
}
