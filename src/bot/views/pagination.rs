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
use crate::bot::views::InteractiveViewBase;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::bot::views::ViewCore;
use crate::bot::views::ViewHandler;

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

pub struct PaginationHandler {
    pub state: PaginationModel,
    pub disabled: bool,
}

#[async_trait::async_trait]
impl ViewHandler<PaginationAction> for PaginationHandler {
    async fn handle(
        &mut self,
        action: &PaginationAction,
        _interaction: &ComponentInteraction,
    ) -> Result<Option<PaginationAction>, Error> {
        match action {
            PaginationAction::First => self.state.first_page(),
            PaginationAction::Prev => self.state.prev_page(),
            PaginationAction::Next => self.state.next_page(),
            PaginationAction::Last => self.state.last_page(),
            _ => return Ok(None),
        }
        Ok(Some(action.clone()))
    }

    /// Disables pagination controls when the view times out.
    async fn on_timeout(&mut self) -> Result<(), Error> {
        self.disabled = true;
        Ok(())
    }
}

/// View that provides pagination controls for multi-page content.
pub struct PaginationView<'a> {
    pub base: InteractiveViewBase<'a, PaginationAction>,
    pub handler: PaginationHandler,
}

impl<'a> View<'a, PaginationAction> for PaginationView<'a> {
    fn core(&self) -> &ViewCore<'a, PaginationAction> {
        &self.base.core
    }
    fn core_mut(&mut self) -> &mut ViewCore<'a, PaginationAction> {
        &mut self.base.core
    }
    fn create_core(poise_ctx: &'a Context<'a>) -> ViewCore<'a, PaginationAction> {
        ViewCore::new(poise_ctx, Duration::from_secs(120))
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
            base: InteractiveViewBase::new(Self::create_core(ctx)),
            handler: PaginationHandler {
                state: model,
                disabled: false,
            },
        }
    }

    /// Attaches pagination controls only if there are multiple pages and not disabled.
    pub fn attach_if_multipage<'b>(&mut self, components: &mut impl Extend<CreateComponent<'b>>) {
        if !self.handler.disabled
            && self.handler.state.pages > 1
            && let ResponseKind::Component(create_components) = self.create_response()
        {
            components.extend(create_components)
        }
    }

    pub fn current_page(&self) -> u32 {
        self.handler.state.current_page
    }
}

impl<'a> ResponseView<'a> for PaginationView<'a> {
    /// Creates the pagination control buttons.
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        if self.handler.disabled {
            return ResponseKind::Component(vec![]);
        }

        let page_label = format!(
            "{}/{}",
            self.handler.state.current_page, self.handler.state.pages
        );

        vec![CreateComponent::ActionRow(CreateActionRow::Buttons(
            vec![
                self.base
                    .register(PaginationAction::First)
                    .as_button()
                    .disabled(self.handler.state.current_page == 1),
                self.base
                    .register(PaginationAction::Prev)
                    .as_button()
                    .disabled(self.handler.state.current_page == 1),
                self.base
                    .register(PaginationAction::Page)
                    .as_button()
                    .label(page_label)
                    .disabled(true)
                    .style(ButtonStyle::Secondary),
                self.base
                    .register(PaginationAction::Next)
                    .as_button()
                    .disabled(self.handler.state.current_page == self.handler.state.pages),
                self.base
                    .register(PaginationAction::Last)
                    .as_button()
                    .disabled(self.handler.state.current_page == self.handler.state.pages),
            ]
            .into(),
        ))]
        .into()
    }
}

crate::impl_interactive_view!(PaginationView<'a>, PaginationHandler, PaginationAction);

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

// ─── V2 Architecture ───────────────────────────────────────────────────────────

use crate::bot::views::ActionRegistry;
use crate::bot::views::Trigger;
use crate::bot::views::ViewCommand;
use crate::bot::views::ViewContextV2;
use crate::bot::views::ViewHandlerV2;

/// The new pagination view for the V2 architecture
#[derive(Clone)]
pub struct PaginationViewV2 {
    pub state: PaginationModel,
    pub disabled: bool,
}

impl PaginationViewV2 {
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

pub struct PaginationHandlerV2;

#[async_trait::async_trait]
impl ViewHandlerV2<PaginationAction> for PaginationViewV2 {
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
            _ => return Ok(ViewCommand::Ignore),
        }
        Ok(ViewCommand::Render)
    }

    async fn on_timeout(&mut self) -> Result<(), Error> {
        self.disabled = true;
        Ok(())
    }
}
