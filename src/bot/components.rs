use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::ComponentInteractionCollector;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;
use serenity::all::CreateInteractionResponse;

use crate::bot::commands::Context;

pub struct Pagination {
    pub current_page: u32,
    pub pages: u32,
    pub per_page: u32,
}

impl Pagination {
    pub fn new(pages: u32, per_page: u32, current_page: u32) -> Self {
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
    }
    pub fn next_page(&mut self) {
        self.current_page += 1;
    }
    pub fn last_page(&mut self) {
        self.current_page = 1;
    }
}

pub struct PageNavigationComponent<'a> {
    pub pagination: Pagination,
    ctx: &'a Context<'a>,
}

impl<'a> PageNavigationComponent<'a> {
    pub fn new(ctx: &'a Context<'a>, pagination: Pagination) -> Self {
        Self { pagination, ctx }
    }

    pub async fn listen(&mut self, timeout: Duration) -> bool {
        let collector =
            ComponentInteractionCollector::new(self.ctx.serenity_context()).timeout(timeout);

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
