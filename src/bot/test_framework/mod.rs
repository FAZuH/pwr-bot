//! Test framework for automated GUI testing of Discord bot views.
//!
//! Provides synthetic events, assertion helpers, and test-step abstractions
//! that drive [`ViewHandler`]s directly without a blocking [`ViewEngine`] loop.

pub mod assert;
pub mod helpers;

use std::fmt::Display;
use std::future::Future;
use std::pin::Pin;

use crate::bot::command::Context;

pub type RunStepResult<'a> = Pin<Box<dyn Future<Output = Result<(), GuiTestError>> + Send + 'a>>;

/// Error type for GUI test failures.
///
/// All failures are caught by the runner and sent as Discord messages.
#[derive(Debug, thiserror::Error)]
pub enum GuiTestError {
    #[error("Step '{step}': assertion failed — expected {expected}, got {actual}")]
    AssertionFailed {
        step: String,
        expected: String,
        actual: String,
    },

    #[error("Step '{step}': setup failed — {detail}")]
    SetupFailed { step: String, detail: String },

    #[error("Step '{step}': execution failed — {detail}")]
    ExecutionFailed { step: String, detail: String },
}

impl GuiTestError {
    /// Constructs an assertion-failed error.
    pub fn assertion_failed(step: impl ToString, expected: impl Display, actual: impl Display) -> Self {
        Self::AssertionFailed {
            step: step.to_string(),
            expected: expected.to_string(),
            actual: actual.to_string(),
        }
    }

    /// Constructs a setup-failed error.
    pub fn setup_failed(step: impl ToString, detail: impl Display) -> Self {
        Self::SetupFailed {
            step: step.to_string(),
            detail: detail.to_string(),
        }
    }

    /// Constructs an execution-failed error.
    pub fn execution_failed(step: impl ToString, detail: impl Display) -> Self {
        Self::ExecutionFailed {
            step: step.to_string(),
            detail: detail.to_string(),
        }
    }
}

/// A single step in the GUI test suite.
pub struct TestStep {
    /// Human-readable name of the command/view under test.
    pub name: &'static str,
    /// Brief description of what this step validates.
    pub description: &'static str,
    /// Async function that executes the test logic.
    pub run: for<'a> fn(Context<'a>) -> RunStepResult<'a>,
}

/// Convenience macro for declaring a [`TestStep`].
#[macro_export]
macro_rules! test_step {
    ($name:expr, $desc:expr, $fn:path) => {
        $crate::bot::test_framework::TestStep {
            name: $name,
            description: $desc,
            run: |ctx| Box::pin($fn(ctx)),
        }
    };
}
