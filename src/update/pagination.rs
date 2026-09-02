//! Pure pagination vocabulary for the core.
//!
//! [`PaginationAction`] is the data-only "user pressed a pagination button"
//! message payload. It carries no labels or UI concerns — those live in the
//! shell's [`crate::bot::view::pagination::PaginationAction`], which the
//! feature's `translate` maps into this pure intent before handing it to
//! `update`.
//!
//! Keeping the pagination *intent* in the core (rather than importing the
//! shell's labelled action enum) preserves the layering rule that
//! `src/update/**` imports no serenity/tokio/diesel/poise and no shell types.

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
