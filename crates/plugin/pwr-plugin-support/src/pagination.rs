//! Pure pagination vocabulary for plugin view cores.
//!
//! [`PaginationAction`] is the message payload for a pagination button. A
//! view core keeps its page math in [`PaginationModel`], while its view layer
//! turns a rendered custom id into the matching action. Keeping the intent in
//! this crate lets a core test navigation without a Discord view or host
//! runtime.

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

/// Pure pagination state shared by a plugin view core and its renderer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaginationModel {
    /// The currently displayed page, one-based.
    pub current_page: u32,
    /// The total number of pages.
    pub pages: u32,
    /// The number of items per page.
    pub per_page: u32,
}

impl PaginationModel {
    /// Creates a pagination model, clamping the current page to its range.
    pub fn new(pages: u32, per_page: u32, current_page: u32) -> Self {
        let pages = pages.max(1);
        let per_page = per_page.max(1);
        Self {
            current_page: current_page.clamp(1, pages),
            pages,
            per_page,
        }
    }

    /// Navigates to the first page.
    pub fn first_page(&mut self) {
        self.current_page = 1;
    }

    /// Navigates to the previous page when one exists.
    pub fn prev_page(&mut self) {
        if self.current_page > 1 {
            self.current_page -= 1;
        }
    }

    /// Navigates to the next page when one exists.
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
        assert_eq!(PaginationModel::new(10, 5, 1).current_page, 1);
        assert_eq!(PaginationModel::new(10, 5, 0).current_page, 1);
        assert_eq!(PaginationModel::new(10, 5, 11).current_page, 10);
        assert_eq!(
            PaginationModel::new(0, 0, 0),
            PaginationModel {
                current_page: 1,
                pages: 1,
                per_page: 1
            }
        );
    }

    #[test]
    fn navigation_stays_in_bounds() {
        let mut pagination = PaginationModel::new(5, 10, 3);
        pagination.prev_page();
        pagination.prev_page();
        pagination.prev_page();
        assert_eq!(pagination.current_page, 1);
        pagination.next_page();
        assert_eq!(pagination.current_page, 2);
        pagination.last_page();
        pagination.next_page();
        assert_eq!(pagination.current_page, 5);
        pagination.first_page();
        assert_eq!(pagination.current_page, 1);
    }

    #[test]
    fn apply_dispatches() {
        let mut pagination = PaginationModel::new(5, 10, 3);
        pagination.apply(PaginationAction::First);
        assert_eq!(pagination.current_page, 1);
        pagination.apply(PaginationAction::Last);
        assert_eq!(pagination.current_page, 5);
        pagination.apply(PaginationAction::Page);
        assert_eq!(pagination.current_page, 5);
    }
}
