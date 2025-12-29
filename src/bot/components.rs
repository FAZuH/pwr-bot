use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::ComponentInteractionCollector;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;
use serenity::all::CreateInteractionResponse;

use crate::bot::commands::Context;

pub struct Pagination {
    /// Current page number. Guaranteed to be >= 1 and <= pages
    pub current_page: u32,
    /// Total number of pages. Guaranteed to be >= 1
    pub pages: u32,
    /// Number of items to show per pag. Guaranteed to be >= 1
    pub per_page: u32,
}

impl Pagination {
    pub fn new(pages: u32, per_page: u32, current_page: u32) -> Self {
        let pages = pages.max(1);
        let per_page = per_page.max(1);
        let current_page = current_page.clamp(1, pages.max(1));
        Self {
            pages,
            per_page,
            current_page,
        }
    }
    pub fn first_page(&mut self) {
        self.current_page = 1;
    }
    pub fn prev_page(&mut self) {
        self.current_page -= 1;
        self.clamp();
    }
    pub fn next_page(&mut self) {
        self.current_page += 1;
        self.clamp();
    }
    pub fn last_page(&mut self) {
        self.current_page = self.pages;
    }
    fn clamp(&mut self) {
        self.current_page = self.current_page.clamp(1, self.pages.max(1));
    }
}

pub struct PageNavigationComponent<'a> {
    pub pagination: Pagination,
    ctx: &'a Context<'a>,
    button_ids: Vec<String>,
}

impl<'a> PageNavigationComponent<'a> {
    pub fn new(ctx: &'a Context<'a>, pagination: Pagination) -> Self {
        let button_ids = vec![
            "first".to_string(),
            "prev".to_string(),
            "next".to_string(),
            "last".to_string(),
        ];
        Self {
            pagination,
            ctx,
            button_ids,
        }
    }

    pub async fn listen(&mut self, timeout: Duration) -> bool {
        let button_ids = self.button_ids.clone();
        let collector = ComponentInteractionCollector::new(self.ctx.serenity_context())
            .filter(move |i| button_ids.contains(&i.data.custom_id.to_string()))
            .timeout(timeout);

        match collector.next().await {
            None => false,
            Some(interaction) => {
                interaction
                    .create_response(self.ctx.http(), CreateInteractionResponse::Acknowledge)
                    .await
                    .ok();

                match interaction.data.custom_id.as_str() {
                    "first" => self.pagination.first_page(),
                    "prev" => self.pagination.prev_page(),
                    "next" => self.pagination.next_page(),
                    "last" => self.pagination.last_page(),
                    _ => {}
                }

                true
            }
        }
    }

    pub fn create_buttons(&self) -> CreateComponent<'_> {
        let page_label = format!("{}/{}", self.pagination.current_page, self.pagination.pages);

        CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                CreateButton::new("first")
                    .label("⏮")
                    .disabled(self.pagination.current_page == 1),
                CreateButton::new("prev")
                    .label("◀")
                    .disabled(self.pagination.current_page == 1),
                CreateButton::new("page")
                    .label(page_label)
                    .disabled(true)
                    .style(ButtonStyle::Secondary),
                CreateButton::new("next")
                    .label("▶")
                    .disabled(self.pagination.current_page == self.pagination.pages),
                CreateButton::new("last")
                    .label("⏭")
                    .disabled(self.pagination.current_page == self.pagination.pages),
            ]
            .into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagination_new() {
        // Normal case
        let p = Pagination::new(10, 5, 1);
        assert_eq!(p.pages, 10);
        assert_eq!(p.per_page, 5);
        assert_eq!(p.current_page, 1);

        // Clamping current_page
        let p = Pagination::new(10, 5, 0);
        assert_eq!(p.current_page, 1);

        let p = Pagination::new(10, 5, 11);
        assert_eq!(p.current_page, 10);

        // Minimal values
        let p = Pagination::new(0, 0, 0);
        assert_eq!(p.pages, 1);
        assert_eq!(p.per_page, 1);
        assert_eq!(p.current_page, 1);
    }

    #[test]
    fn test_pagination_navigation() {
        let mut p = Pagination::new(5, 10, 3);

        p.prev_page();
        assert_eq!(p.current_page, 2);

        p.prev_page();
        assert_eq!(p.current_page, 1);

        p.prev_page();
        assert_eq!(p.current_page, 1); // Should not go below 1

        p.next_page();
        assert_eq!(p.current_page, 2);

        p.last_page();
        assert_eq!(p.current_page, 5);

        p.next_page();
        assert_eq!(p.current_page, 5); // Should not go above pages

        p.first_page();
        assert_eq!(p.current_page, 1);
    }
}
