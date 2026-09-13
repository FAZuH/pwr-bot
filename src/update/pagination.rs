//! Pure pagination vocabulary for the core.
//!
//! [`PaginationAction`] is the single "user pressed a pagination button"
//! message payload, shared by the core and the shell: the shell re-exports
//! this type through [`crate::bot::view::pagination`] and adds the UI
//! [`Action`](crate::bot::view::Action) labels plus the
//! [`PaginationView`](crate::bot::view::pagination::PaginationView) row
//! renderer there, while a feature's `translate` maps the fired action
//! straight into this intent before handing it to `update`.
//!
//! Keeping the pagination *intent* in the core (rather than defining it in
//! the shell) preserves the layering rule that `src/update/**` imports no
//! serenity/tokio/diesel/poise and no shell types.

/// Which pagination navigation the user requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaginationAction {
    /// Jump to the first page.
    First,
    /// Go to the previous page.
    Prev,
    /// Go to the next page.
    Next,
    /// Jump to the last page.
    Last,
    /// Select a specific page (unused by the current views).
    Page,
}

/// Pure pagination state — the single source of truth for a paged view.
///
/// Shared by the core and the shell (re-exported through
/// [`crate::bot::view::pagination`]) so a feature's page math can live in the
/// core and be unit-tested without the shell. The shell's
/// [`PaginationView`](crate::bot::view::pagination::PaginationView) is fed
/// from this state when it renders the row, keeping the rendered buttons
/// byte-identical.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaginationModel {
    /// The currently displayed page (1-based).
    pub current_page: u32,
    /// The total number of pages.
    pub pages: u32,
    /// The number of items per page.
    pub per_page: u32,
}

impl PaginationModel {
    /// Creates a pagination model, clamping the current page to `1..=pages`.
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

    /// Navigates to the previous page, if not already on the first.
    pub fn prev_page(&mut self) {
        if self.current_page > 1 {
            self.current_page -= 1;
        }
    }

    /// Navigates to the next page, if not already on the last.
    pub fn next_page(&mut self) {
        if self.current_page < self.pages {
            self.current_page += 1;
        }
    }

    /// Navigates to the last page.
    pub fn last_page(&mut self) {
        self.current_page = self.pages;
    }

    /// Applies a pagination intent in place.
    pub fn apply(&mut self, action: PaginationAction) {
        match action {
            PaginationAction::First => self.first_page(),
            PaginationAction::Prev => self.prev_page(),
            PaginationAction::Next => self.next_page(),
            PaginationAction::Last => self.last_page(),
            PaginationAction::Page => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clamps() {
        let p = PaginationModel::new(10, 5, 1);
        assert_eq!((p.pages, p.per_page, p.current_page), (10, 5, 1));

        let p = PaginationModel::new(10, 5, 0);
        assert_eq!(p.current_page, 1);

        let p = PaginationModel::new(10, 5, 11);
        assert_eq!(p.current_page, 10);

        let p = PaginationModel::new(0, 0, 0);
        assert_eq!((p.pages, p.per_page, p.current_page), (1, 1, 1));
    }

    #[test]
    fn navigation() {
        let mut p = PaginationModel::new(5, 10, 3);

        p.prev_page();
        assert_eq!(p.current_page, 2);
        p.prev_page();
        assert_eq!(p.current_page, 1);
        p.prev_page();
        assert_eq!(p.current_page, 1); // stuck at 1

        p.next_page();
        assert_eq!(p.current_page, 2);
        p.last_page();
        assert_eq!(p.current_page, 5);
        p.next_page();
        assert_eq!(p.current_page, 5); // stuck at 5
        p.first_page();
        assert_eq!(p.current_page, 1);
    }

    #[test]
    fn apply_dispatches() {
        let mut p = PaginationModel::new(5, 10, 3);
        p.apply(PaginationAction::First);
        assert_eq!(p.current_page, 1);
        p.apply(PaginationAction::Last);
        assert_eq!(p.current_page, 5);
        p.apply(PaginationAction::Page); // no-op
        assert_eq!(p.current_page, 5);
    }
}
