use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::ComponentInteractionCollector;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;
use serenity::all::CreateInteractionResponse;

use crate::bot::commands::Context;

pub struct PaginationState {
    pub current_page: u32,
    pub pages: u32,
    pub per_page: u32,
}

impl PaginationState {
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
        if self.current_page > 1 {
            self.current_page -= 1;
        }
    }

    pub fn next_page(&mut self) {
        if self.current_page < self.pages {
            self.current_page += 1;
        }
    }

    pub fn last_page(&mut self) {
        self.current_page = self.pages;
    }

    pub fn create_buttons(&self) -> CreateComponent<'static> {
        let page_label = format!("{}/{}", self.current_page, self.pages);

        CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                CreateButton::new("first")
                    .label("⏮")
                    .disabled(self.current_page == 1),
                CreateButton::new("prev")
                    .label("◀")
                    .disabled(self.current_page == 1),
                CreateButton::new("page")
                    .label(page_label)
                    .disabled(true)
                    .style(ButtonStyle::Secondary),
                CreateButton::new("next")
                    .label("▶")
                    .disabled(self.current_page == self.pages),
                CreateButton::new("last")
                    .label("⏭")
                    .disabled(self.current_page == self.pages),
            ]
            .into(),
        ))
    }
}

pub struct PaginationHandler<'a> {
    ctx: &'a Context<'a>,
    state: PaginationState,
}

impl<'a> PaginationHandler<'a> {
    const BUTTON_IDS: [&'static str; 4] = ["first", "prev", "next", "last"];

    pub fn new(ctx: &'a Context<'a>, state: PaginationState) -> Self {
        Self { ctx, state }
    }

    pub fn state(&self) -> &PaginationState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut PaginationState {
        &mut self.state
    }

    pub async fn listen(&mut self, timeout: Duration) -> Option<&'static str> {
        let collector = ComponentInteractionCollector::new(self.ctx.serenity_context())
            .author_id(self.ctx.author().id)
            .filter(move |i| Self::BUTTON_IDS.contains(&i.data.custom_id.as_str()))
            .timeout(timeout);

        match collector.next().await {
            None => None,
            Some(interaction) => {
                interaction
                    .create_response(self.ctx.http(), CreateInteractionResponse::Acknowledge)
                    .await
                    .ok();

                match interaction.data.custom_id.as_str() {
                    "first" => {
                        self.state.first_page();
                        Some("first")
                    }
                    "prev" => {
                        self.state.prev_page();
                        Some("prev")
                    }
                    "next" => {
                        self.state.next_page();
                        Some("next")
                    }
                    "last" => {
                        self.state.last_page();
                        Some("last")
                    }
                    _ => None,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagination_new() {
        // Normal case
        let p = PaginationState::new(10, 5, 1);
        assert_eq!(p.pages, 10);
        assert_eq!(p.per_page, 5);
        assert_eq!(p.current_page, 1);

        // Clamping current_page
        let p = PaginationState::new(10, 5, 0);
        assert_eq!(p.current_page, 1);

        let p = PaginationState::new(10, 5, 11);
        assert_eq!(p.current_page, 10);

        // Minimal values
        let p = PaginationState::new(0, 0, 0);
        assert_eq!(p.pages, 1);
        assert_eq!(p.per_page, 1);
        assert_eq!(p.current_page, 1);
    }

    #[test]
    fn test_pagination_navigation() {
        let mut p = PaginationState::new(5, 10, 3);

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
