//! Assertion helpers for GUI test steps.

use crate::bot::coordinator::Coordinator;
use crate::bot::navigation::Navigation;
use crate::bot::test_framework::GuiTestError;
use crate::bot::view::Action;
use crate::bot::view::ActionRegistry;
use crate::bot::view::ViewCommand;

/// Finds an action in the registry by its label.
pub fn assert_has_action<T: Action + Clone>(
    registry: &ActionRegistry<T>,
    label: &str,
) -> Result<T, GuiTestError> {
    registry
        .actions
        .values()
        .find(|a| a.label() == label)
        .cloned()
        .ok_or_else(|| {
            GuiTestError::assertion_failed(
                "render",
                format!("action with label '{}'", label),
                "not found",
            )
        })
}

/// Asserts that the coordinator's most recent navigation target matches.
pub fn assert_navigated_to(
    coordinator: &Coordinator,
    expected: Navigation,
) -> Result<(), GuiTestError> {
    let actual = coordinator.peek_navigation();
    if actual.as_ref() == Some(&expected) {
        Ok(())
    } else {
        Err(GuiTestError::assertion_failed(
            "navigation",
            format!("{:?}", expected),
            format!("{:?}", actual),
        ))
    }
}

/// Asserts that two [`ViewCommand`]s are equal.
pub fn assert_eq_cmd(
    actual: ViewCommand,
    expected: ViewCommand,
    msg: &str,
) -> Result<(), GuiTestError> {
    if actual == expected {
        Ok(())
    } else {
        Err(GuiTestError::assertion_failed(
            msg,
            format!("{:?}", expected),
            format!("{:?}", actual),
        ))
    }
}
