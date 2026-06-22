//! Interactive feed list view rendering.
//!
//! Builds Discord Components V2 messages matching the main branch's feed
//! list UI: sections with thumbnails, pagination, edit/view toggle, and
//! per-feed Unsubscribe/Undo buttons in edit mode.

use pwr_bot_sdk::*;
use serenity::all::*;

/// Number of feeds per page in view mode.
pub const PER_PAGE: u32 = 5;

/// Number of feeds per page in edit mode (fewer to make room for buttons).
pub const EDIT_PER_PAGE: u32 = 8;

/// Prefix for all custom IDs in the feed list view.
pub const CUSTOM_ID_PREFIX: &str = "feed:";

/// State for the interactive feed list view, keyed by author_id.
#[derive(Clone)]
pub struct FeedListState {
    pub page: u32,
    pub total_pages: u32,
    pub edit_mode: bool,
    pub subscriber_id: i32,
    pub selected_unsub: Vec<i32>,
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
    let mut components: Vec<CreateComponent<'_>> = Vec::new();

    if feeds.is_empty() {
        components.push(CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                "You have no subscriptions.",
            )),
        ])));
    } else {
        // Build sections for each feed matching the main branch layout
        let mut sections: Vec<CreateContainerComponent<'_>> = Vec::new();
        for feed in feeds {
            let name = feed
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let source_url = feed
                .get("source_url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let cover_url = feed.get("cover_url").and_then(|v| v.as_str()).unwrap_or("");
            let latest_title = feed.get("latest_title").and_then(|v| v.as_str());
            let latest_published = feed.get("latest_published").and_then(|v| v.as_i64());

            let text = if let (Some(title), Some(ts)) = (latest_title, latest_published) {
                format!(
                    "### {name}\n\n- **Last version**: {title}\n- **Last updated**: <t:{ts}>\n- [**Source** 🗗](<{source_url}>)",
                )
            } else {
                format!(
                    "### {name}\n\n> No latest version found.\n- [**Source** 🗗](<{source_url}>)",
                )
            };

            let section = CreateContainerComponent::Section(CreateSection::new(
                vec![CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                    text,
                ))],
                CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                    CreateUnfurledMediaItem::new(cover_url),
                )),
            ));
            sections.push(section);
        }

        components.push(CreateComponent::Container(CreateContainer::new(sections)));

        // Pagination buttons (if multi-page)
        if state.total_pages > 1 {
            let pg = state.page;
            let total = state.total_pages;

            let mut first = CreateButton::new(format!("{CUSTOM_ID_PREFIX}pg:1"))
                .label("\u{23ee}")
                .style(ButtonStyle::Primary);
            let mut prev =
                CreateButton::new(format!("{CUSTOM_ID_PREFIX}pg:{}", pg.saturating_sub(1)))
                    .label("\u{25c0}")
                    .style(ButtonStyle::Primary);
            let current = CreateButton::new(format!("{CUSTOM_ID_PREFIX}pg:current"))
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

            components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
                vec![first, prev, current, next, last].into(),
            )));
        }
    }

    // Toggle row: [Edit Subscriptions button]
    let edit_btn = CreateButton::new(format!("{CUSTOM_ID_PREFIX}edit"))
        .label("\u{270e} Edit Subscriptions")
        .style(ButtonStyle::Primary);
    components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
        vec![edit_btn].into(),
    )));

    let create_message = CreateMessage::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(components);

    ResponsePayload::from_serializable(&create_message)
        .unwrap_or_else(|_| ResponsePayload::text("Error rendering feed list"))
}

fn render_edit_mode(state: &FeedListState, feeds: &[serde_json::Value]) -> ResponsePayload {
    let mut components: Vec<CreateComponent<'_>> = Vec::new();

    if feeds.is_empty() {
        components.push(CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                "You have no subscriptions to edit.",
            )),
        ])));
    } else {
        // Build sections with per-feed Unsubscribe/Undo buttons
        let mut sections: Vec<CreateContainerComponent<'_>> = Vec::new();
        for feed in feeds {
            let feed_id = feed.get("id").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let name = feed
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let source_url = feed
                .get("source_url")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let latest_title = feed.get("latest_title").and_then(|v| v.as_str());
            let latest_published = feed.get("latest_published").and_then(|v| v.as_i64());

            let text = if let (Some(title), Some(ts)) = (latest_title, latest_published) {
                format!(
                    "### {name}\n\n- **Last version**: {title}\n- **Last updated**: <t:{ts}>\n- [**Source** 🗗](<{source_url}>)",
                )
            } else {
                format!(
                    "### {name}\n\n> No latest version found.\n- [**Source** 🗗](<{source_url}>)",
                )
            };

            let is_marked = state.selected_unsub.contains(&feed_id);
            let button = if is_marked {
                CreateButton::new(format!("{CUSTOM_ID_PREFIX}undo:{feed_id}"))
                    .label("\u{21a9} Undo")
                    .style(ButtonStyle::Secondary)
            } else {
                CreateButton::new(format!("{CUSTOM_ID_PREFIX}unsub:{feed_id}"))
                    .label("\u{1f5d1} Unsubscribe")
                    .style(ButtonStyle::Danger)
            };

            let section = CreateContainerComponent::Section(CreateSection::new(
                vec![CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                    text,
                ))],
                CreateSectionAccessory::Button(button),
            ));
            sections.push(section);
        }

        components.push(CreateComponent::Container(CreateContainer::new(sections)));
    }

    // Toggle row: [View Mode, Save]
    let view_btn = CreateButton::new(format!("{CUSTOM_ID_PREFIX}view"))
        .label("\u{1f441} View Mode")
        .style(ButtonStyle::Primary);
    let mut save_btn = CreateButton::new(format!("{CUSTOM_ID_PREFIX}save"))
        .label("Save")
        .style(ButtonStyle::Success);
    if state.selected_unsub.is_empty() {
        save_btn = save_btn.disabled(true);
    }

    components.push(CreateComponent::ActionRow(CreateActionRow::Buttons(
        vec![view_btn, save_btn].into(),
    )));

    let create_message = CreateMessage::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(components);

    ResponsePayload::from_serializable(&create_message)
        .unwrap_or_else(|_| ResponsePayload::text("Error rendering edit mode"))
}
