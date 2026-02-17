//! Pagination component for Discord views.

use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::ComponentInteraction;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;

use crate::action_enum;
use crate::bot::Error;
use crate::bot::commands::Context;
use crate::bot::views::InteractiveView;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::view_core;

/// Model for tracking pagination state.
pub struct PaginationModel {
    pub current_page: u32,
    pub pages: u32,
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
    First,
    Prev,
    Page,
    Next,
    Last,
});

view_core! {
    timeout = Duration::from_secs(120),
    /// View that provides pagination controls for multi-page content.
    pub struct PaginationView<'a, PaginationAction> {
        pub state: PaginationModel,
        pub disabled: bool,
    }
}

impl<'a> PaginationView<'a> {
    /// Creates a new pagination view with the given item count and page size.
    pub fn new(
        ctx: &'a Context<'a>,
        total_items: impl Into<u32>,
        per_page: impl Into<u32>,
    ) -> Self {
        let per_page = per_page.into();
        let pages = total_items.into().div_ceil(per_page);
        let model = PaginationModel::new(pages, per_page, 1);
        Self {
            state: model,
            disabled: false,
            core: Self::create_core(ctx),
        }
    }

    /// Attaches pagination controls only if there are multiple pages and not disabled.
    pub fn attach_if_multipage<'b>(&mut self, components: &mut impl Extend<CreateComponent<'b>>) {
        if !self.disabled
            && self.state.pages > 1
            && let ResponseKind::Component(create_components) = self.create_response()
        {
            components.extend(create_components)
        }
    }
}

impl<'a> ResponseView<'a> for PaginationView<'a> {
    /// Creates the pagination control buttons.
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        if self.disabled {
            return ResponseKind::Component(vec![]);
        }

        let page_label = format!("{}/{}", self.state.current_page, self.state.pages);

        vec![CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                CreateButton::new(self.register(PaginationAction::First))
                    .label("⏮")
                    .disabled(self.state.current_page == 1),
                CreateButton::new(self.register(PaginationAction::Prev))
                    .label("◀")
                    .disabled(self.state.current_page == 1),
                CreateButton::new(self.register(PaginationAction::Page))
                    .label(page_label)
                    .disabled(true)
                    .style(ButtonStyle::Secondary),
                CreateButton::new(self.register(PaginationAction::Next))
                    .label("▶")
                    .disabled(self.state.current_page == self.state.pages),
                CreateButton::new(self.register(PaginationAction::Last))
                    .label("⏭")
                    .disabled(self.state.current_page == self.state.pages),
            ]
            .into(),
        ))]
        .into()
    }
}

#[async_trait::async_trait]
impl<'a> InteractiveView<'a, PaginationAction> for PaginationView<'a> {
    async fn handle(
        &mut self,
        action: &PaginationAction,
        _interaction: &ComponentInteraction,
    ) -> Option<PaginationAction> {
        match action {
            PaginationAction::First => self.state.first_page(),
            PaginationAction::Prev => self.state.prev_page(),
            PaginationAction::Next => self.state.next_page(),
            PaginationAction::Last => self.state.last_page(),
            _ => return None,
        }
        Some(action.clone())
    }

    /// Disables pagination controls when the view times out.
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
