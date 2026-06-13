pub mod feed_list;
pub mod feed_settings;

/// Simple pagination action (no ViewEngine dependency).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaginationAction {
    First,
    Prev,
    Next,
    Last,
    Page,
}

/// Simple pagination model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaginationModel {
    pub current_page: u32,
    pub pages: u32,
}

impl PaginationModel {
    pub fn new(pages: u32, current_page: u32) -> Self {
        Self {
            pages: pages.max(1),
            current_page: current_page.clamp(1, pages.max(1)),
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
