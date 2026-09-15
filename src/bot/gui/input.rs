//! Custom-id helpers for the interactive view layer.
//!
//! Extracted from the [`crate::bot::view`] registry flow. Components built by a
//! feature's `view` register actions against an [`ActionRegistry`], which
//! assigns each a `Type:timestamp:counter` custom id. On fire, the host
//! resolves the id back to its action through the same registry.

use crate::bot::view::Action;
use crate::bot::view::ActionRegistry;
use crate::bot::view::RegisteredAction;

/// The separator used in the `Type:timestamp:counter` scheme.
pub const CUSTOM_ID_SEP: char = ':';

/// Builds a custom id from its parts. Matches the scheme produced by
/// [`ActionRegistry::register`]: `Type:timestamp:counter`.
pub fn build_custom_id(kind: &str, timestamp: u128, counter: usize) -> String {
    format!("{kind}{CUSTOM_ID_SEP}{timestamp}{CUSTOM_ID_SEP}{counter}")
}

/// Splits a `Type:timestamp:counter` custom id into its parts.
///
/// Returns `None` when the id does not match the scheme (e.g. a raw custom id
/// that was never register-generated).
pub fn parse_custom_id(id: &str) -> Option<(&str, &str, &str)> {
    let mut parts = id.splitn(3, CUSTOM_ID_SEP);
    let kind = parts.next()?;
    let timestamp = parts.next()?;
    let counter = parts.next()?;
    if kind.is_empty() || timestamp.is_empty() || counter.is_empty() {
        return None;
    }
    Some((kind, timestamp, counter))
}

/// Registers an action and returns its [`RegisteredAction`] for a view.
pub fn register<T: Action>(registry: &mut ActionRegistry<T>, action: T) -> RegisteredAction {
    registry.register(action)
}

/// Resolves a fired custom id back to its registered action, if any.
pub fn resolve<T: Action + Clone>(registry: &ActionRegistry<T>, custom_id: &str) -> Option<T> {
    registry.get(custom_id).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_parse_roundtrip() {
        let id = build_custom_id("AboutAction", 1_700_000_000_000, 0);
        let parsed = parse_custom_id(&id);
        assert_eq!(parsed, Some(("AboutAction", "1700000000000", "0")));
    }

    #[test]
    fn parse_rejects_non_scheme_id() {
        assert_eq!(parse_custom_id("raw-custom-id"), None);
    }
}
