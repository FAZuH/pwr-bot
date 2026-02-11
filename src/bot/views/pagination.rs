//! Pagination component for Discord views.

use std::str::FromStr;
use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::ComponentInteraction;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;

use crate::bot::commands::Context;
use crate::bot::views::Action;
use crate::bot::views::InteractableComponentView;
use crate::bot::views::ResponseComponentView;
use crate::custom_id_enum;
use crate::stateful_view;

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

    /// Creates the pagination control buttons.
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

custom_id_enum!(PaginationAction {
    First,
    Prev,
    Page,
    Next,
    Last,
});

stateful_view! {
    timeout = Duration::from_secs(120),
    /// View that provides pagination controls for multi-page content.
    pub struct PaginationView<'a> {
        pub state: PaginationModel,
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
            ctx: Self::create_context(ctx),
        }
    }

    /// Creates a pagination view from an existing model.
    pub fn from_model(ctx: &'a Context<'a>, model: PaginationModel) -> Self {
        Self {
            state: model,
            ctx: Self::create_context(ctx),
        }
    }

    /// Attaches pagination controls only if there are multiple pages.
    pub fn attach_if_multipage<'b>(&self, components: &mut impl Extend<CreateComponent<'b>>) {
        if self.state.pages > 1 {
            ResponseComponentView::attach(self, components);
        }
    }
}

impl<'a> ResponseComponentView for PaginationView<'a> {
    fn create_components<'b>(&self) -> Vec<CreateComponent<'b>> {
        let page_label = format!("{}/{}", self.state.current_page, self.state.pages);

        vec![CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                CreateButton::new(PaginationAction::First.custom_id())
                    .label("⏮")
                    .disabled(self.state.current_page == 1),
                CreateButton::new(PaginationAction::Prev.custom_id())
                    .label("◀")
                    .disabled(self.state.current_page == 1),
                CreateButton::new(PaginationAction::Page.custom_id())
                    .label(page_label)
                    .disabled(true)
                    .style(ButtonStyle::Secondary),
                CreateButton::new(PaginationAction::Next.custom_id())
                    .label("▶")
                    .disabled(self.state.current_page == self.state.pages),
                CreateButton::new(PaginationAction::Last.custom_id())
                    .label("⏭")
                    .disabled(self.state.current_page == self.state.pages),
            ]
            .into(),
        ))]
    }
}

#[async_trait::async_trait]
impl<'a> InteractableComponentView<'a, PaginationAction> for PaginationView<'a> {
    async fn handle(&mut self, interaction: &ComponentInteraction) -> Option<PaginationAction> {
        let action = PaginationAction::from_str(&interaction.data.custom_id).ok()?;

        match action {
            PaginationAction::First => self.state.first_page(),
            PaginationAction::Prev => self.state.prev_page(),
            PaginationAction::Next => self.state.next_page(),
            PaginationAction::Last => self.state.last_page(),
            _ => return None,
        }

        Some(action)
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
