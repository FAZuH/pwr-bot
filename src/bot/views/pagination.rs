//! Pagination component for Discord views.

use serenity::all::ButtonStyle;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;

use crate::action_enum;
use crate::bot::Error;

/// Model for tracking pagination state.
#[derive(Clone)]
pub struct PaginationModel {
    pub current_page: u32,
    pub pages: u32,
    #[allow(dead_code)]
    pub per_page: u32,
}

impl PaginationModel {
    /// Creates a new pagination model with the given parameters.
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

    /// Navigates to the first page.
    pub fn first_page(&mut self) {
        self.current_page = 1;
    }

    /// Navigates to the previous page if not on the first page.
    pub fn prev_page(&mut self) {
        if self.current_page > 1 {
            self.current_page -= 1;
        }
    }

    /// Navigates to the next page if not on the last page.
    pub fn next_page(&mut self) {
        if self.current_page < self.pages {
            self.current_page += 1;
        }
    }

    /// Navigates to the last page.
    pub fn last_page(&mut self) {
        self.current_page = self.pages;
    }
}

action_enum!(PaginationAction {
    #[label = "⏮"]
    First,
    #[label = "◀"]
    Prev,
    Page,
    #[label = "▶"]
    Next,
    #[label = "⏭"]
    Last,
});

use crate::bot::views::ActionRegistry;
use crate::bot::views::Trigger;
use crate::bot::views::ViewCommand;
use crate::bot::views::ViewContextV2;
use crate::bot::views::ViewHandlerV2;

#[derive(Clone)]
pub struct PaginationView {
    pub state: PaginationModel,
    pub disabled: bool,
}

impl PaginationView {
    pub fn new(total_items: impl Into<u32>, per_page: impl Into<u32>) -> Self {
        let per_page = per_page.into();
        let pages = total_items.into().div_ceil(per_page);
        let model = PaginationModel::new(pages, per_page, 1);
        Self {
            state: model,
            disabled: false,
        }
    }

    pub fn current_page(&self) -> u32 {
        self.state.current_page
    }

    pub fn attach_if_multipage<'b, T: crate::bot::views::Action>(
        &self,
        registry: &mut ActionRegistry<T>,
        components: &mut Vec<CreateComponent<'b>>,
        wrap: fn(PaginationAction) -> T,
    ) {
        if !self.disabled && self.state.pages > 1 {
            components.push(self.create_component(registry, wrap));
        }
    }

    pub fn create_component<'b, T: crate::bot::views::Action>(
        &self,
        registry: &mut ActionRegistry<T>,
        wrap: fn(PaginationAction) -> T,
    ) -> CreateComponent<'b> {
        let mut first = crate::bot::views::RegisteredAction {
            id: registry.register(wrap(PaginationAction::First)),
            label: "⏮",
        }
        .as_button()
        .style(ButtonStyle::Primary);
        let mut prev = crate::bot::views::RegisteredAction {
            id: registry.register(wrap(PaginationAction::Prev)),
            label: "◀",
        }
        .as_button()
        .style(ButtonStyle::Primary);
        let current = CreateButton::new("current")
            .label(format!("{}/{}", self.state.current_page, self.state.pages))
            .style(ButtonStyle::Secondary)
            .disabled(true);
        let mut next = crate::bot::views::RegisteredAction {
            id: registry.register(wrap(PaginationAction::Next)),
            label: "▶",
        }
        .as_button()
        .style(ButtonStyle::Primary);
        let mut last = crate::bot::views::RegisteredAction {
            id: registry.register(wrap(PaginationAction::Last)),
            label: "⏭",
        }
        .as_button()
        .style(ButtonStyle::Primary);

        if self.state.current_page == 1 || self.disabled {
            first = first.disabled(true);
            prev = prev.disabled(true);
        }
        if self.state.current_page == self.state.pages || self.disabled {
            next = next.disabled(true);
            last = last.disabled(true);
        }

        CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![first, prev, current, next, last].into(),
        ))
    }
}

#[async_trait::async_trait]
impl ViewHandlerV2<PaginationAction> for PaginationView {
    async fn handle(
        &mut self,
        action: PaginationAction,
        _trigger: Trigger<'_>,
        _ctx: &ViewContextV2<'_, PaginationAction>,
    ) -> Result<ViewCommand, Error> {
        match action {
            PaginationAction::First => self.state.first_page(),
            PaginationAction::Prev => self.state.prev_page(),
            PaginationAction::Next => self.state.next_page(),
            PaginationAction::Last => self.state.last_page(),
            _ => return Ok(ViewCommand::Continue),
        }
        Ok(ViewCommand::Render)
    }

    async fn on_timeout(&mut self) -> Result<(), Error> {
        self.disabled = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagination_new() {
        // Normal case
        let p = PaginationModel::new(10, 5, 1);
        assert_eq!(p.pages, 10);
        assert_eq!(p.per_page, 5);
        assert_eq!(p.current_page, 1);

        // Clamping current_page
        let p = PaginationModel::new(10, 5, 0);
        assert_eq!(p.current_page, 1);

        let p = PaginationModel::new(10, 5, 11);
        assert_eq!(p.current_page, 10);

        // Minimal values
        let p = PaginationModel::new(0, 0, 0);
        assert_eq!(p.pages, 1);
        assert_eq!(p.per_page, 1);
        assert_eq!(p.current_page, 1);
    }

    #[test]
    fn test_pagination_navigation() {
        let mut p = PaginationModel::new(5, 10, 3);

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
