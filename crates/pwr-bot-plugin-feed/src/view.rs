//! Interactive feed list view rendering.
//!
//! Builds Discord Components V2 messages for the feed list, supporting
//! pagination and an edit mode with a select menu for unsubscribing.

use pwr_bot_sdk::*;
use serenity::all::*;

/// Number of feeds per page in view mode.
pub const PER_PAGE: u32 = 5;

/// Prefix for all custom IDs in the feed list view.
pub const CUSTOM_ID_PREFIX: &str = "feed:";

/// State for the interactive feed list view, keyed by author_id.
#[derive(Clone)]
pub struct FeedListState {
    pub page: u32,
    pub total_pages: u32,
    pub edit_mode: bool,
    pub subscriber_id: i32,
    pub selected_unsub: Vec<String>,
}

impl FeedListState {
    pub fn new(subscriber_id: i32, total_pages: u32) -> Self {
        Self {
            page: 1,
            total_pages,
            edit_mode: false,
            subscriber_id,
            selected_unsub: Vec::new(),
        }
    }
}

/// Renders the feed list view as a Components V2 [`ResponsePayload`].
pub fn render_feed_list(state: &FeedListState, feeds: &[serde_json::Value]) -> ResponsePayload {
    if state.edit_mode {
        render_edit_mode(state, feeds)
    } else {
        render_view_mode(state, feeds)
    }
}

fn render_view_mode(state: &FeedListState, feeds: &[serde_json::Value]) -> ResponsePayload {
    let mut text = String::from("## Your Subscriptions\n\n");

    if feeds.is_empty() {
        text.push_str("No subscriptions on this page.");
    } else {
        for (i, feed) in feeds.iter().enumerate() {
            let name = feed
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let source_url = feed
                .get("source_url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let platform = feed
                .get("platform_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let tags = feed.get("tags").and_then(|v| v.as_str()).unwrap_or("");

            let line = format!(
                "{}. **[{name}](<{source_url}>)** ({platform})\n",
                (state.page - 1) * PER_PAGE + i as u32 + 1
            );
            text.push_str(&line);

            if !tags.is_empty() {
                text.push_str(&format!("   └ Tags: {tags}\n"));
            }
        }
    }

    let mut container_components: Vec<CreateContainerComponent> = Vec::new();

    container_components.push(CreateContainerComponent::TextDisplay(
        CreateTextDisplay::new(text),
    ));

    // Pagination buttons (if multi-page)
    if state.total_pages > 1 {
        let pg = state.page;
        let total = state.total_pages;

        let mut first = CreateButton::new(format!("{CUSTOM_ID_PREFIX}pg:1"))
            .label("\u{23ee}")
            .style(ButtonStyle::Primary);
        let mut prev = CreateButton::new(format!("{CUSTOM_ID_PREFIX}pg:{}", pg.saturating_sub(1)))
            .label("\u{25c0}")
            .style(ButtonStyle::Primary);
        let current = CreateButton::new("feed:pg:current")
            .label(format!("{pg}/{total}"))
            .style(ButtonStyle::Secondary)
            .disabled(true);
        let mut next = CreateButton::new(format!("{CUSTOM_ID_PREFIX}pg:{}", pg + 1))
            .label("\u{25b6}")
            .style(ButtonStyle::Primary);
        let mut last = CreateButton::new(format!("{CUSTOM_ID_PREFIX}pg:{total}"))
            .label("\u{23ed}")
            .style(ButtonStyle::Primary);

        if pg <= 1 {
            first = first.disabled(true);
            prev = prev.disabled(true);
        }
        if pg >= total {
            next = next.disabled(true);
            last = last.disabled(true);
        }

        container_components.push(CreateContainerComponent::ActionRow(
            CreateActionRow::Buttons(vec![first, prev, current, next, last].into()),
        ));
    }

    // Edit button
    let edit_btn = CreateButton::new(format!("{CUSTOM_ID_PREFIX}edit"))
        .label("Edit")
        .style(ButtonStyle::Secondary);

    container_components.push(CreateContainerComponent::ActionRow(
        CreateActionRow::Buttons(vec![edit_btn].into()),
    ));

    let container = CreateContainer::new(container_components);
    let create_message = CreateMessage::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(vec![CreateComponent::Container(container)]);

    ResponsePayload::from_serializable(&create_message)
        .unwrap_or_else(|_| ResponsePayload::text("Error rendering feed list"))
}

fn render_edit_mode(state: &FeedListState, feeds: &[serde_json::Value]) -> ResponsePayload {
    let mut container_components: Vec<CreateContainerComponent> = Vec::new();

    let text = if feeds.is_empty() {
        "## Edit Subscriptions\n\nYou have no subscriptions to edit."
    } else {
        "## Edit Subscriptions\n\nSelect feeds to unsubscribe from the dropdown below, then click **Save**."
    };

    container_components.push(CreateContainerComponent::TextDisplay(
        CreateTextDisplay::new(text),
    ));

    // Select menu with feed options (up to 25 — Discord limit)
    let options: Vec<CreateSelectMenuOption> = feeds
        .iter()
        .take(25)
        .filter_map(|feed| {
            let feed_id = feed.get("id").and_then(|v| v.as_i64())?;
            let name = feed
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let platform = feed
                .get("platform_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let mut opt =
                CreateSelectMenuOption::new(format!("{name} ({platform})"), feed_id.to_string());
            if state.selected_unsub.contains(&feed_id.to_string()) {
                opt = opt.default_selection(true);
            }
            Some(opt)
        })
        .collect();

    if !options.is_empty() {
        let opt_count = options.len() as u8;
        let select_menu = CreateSelectMenu::new(
            format!("{CUSTOM_ID_PREFIX}select"),
            CreateSelectMenuKind::String {
                options: options.into(),
            },
        )
        .placeholder("Select feeds to unsubscribe")
        .min_values(0)
        .max_values(opt_count);

        container_components.push(CreateContainerComponent::ActionRow(
            CreateActionRow::SelectMenu(select_menu),
        ));
    }

    // Save and Cancel buttons
    let save_btn = CreateButton::new(format!("{CUSTOM_ID_PREFIX}save"))
        .label("Save")
        .style(ButtonStyle::Danger);
    let cancel_btn = CreateButton::new(format!("{CUSTOM_ID_PREFIX}cancel"))
        .label("Cancel")
        .style(ButtonStyle::Secondary);

    container_components.push(CreateContainerComponent::ActionRow(
        CreateActionRow::Buttons(vec![save_btn, cancel_btn].into()),
    ));

    let container = CreateContainer::new(container_components);
    let create_message = CreateMessage::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(vec![CreateComponent::Container(container)]);

    ResponsePayload::from_serializable(&create_message)
        .unwrap_or_else(|_| ResponsePayload::text("Error rendering edit mode"))
}
