//! Controller pattern for managing interactive command flows.
//!
//! This module provides the Controller trait and Coordinator for managing
//! sequences of interactive controllers in Discord commands.

use std::future::Future;

use crate::bot::commands::Error;

/// Core trait for ALL controllers.
///
/// Controllers manage an interactive flow and produce output when complete.
/// Use with [`Coordinator`] to chain multiple controllers together.
///
/// # Type Parameters
///
/// - `O`: The output type produced when this controller completes
///
/// # Example
    ///
    /// ```ignore
    /// struct SettingsController<'a> {
    ///     ctx: &'a Context<'a>,
    /// }
    ///
    /// impl<'a> Controller<SettingsResult> for SettingsController<'a> {
    ///     async fn run(&mut self) -> Result<SettingsResult, Error> {
    ///         // Interactive flow logic here
    ///         Ok(SettingsResult::Saved)
    ///     }
    /// }
    /// ```
pub trait Controller<O> {
    /// Runs this controller until completion.
    ///
    /// This method executes the controller's interactive flow, handling
    /// user interactions until the controller decides to exit.
    ///
    /// # Returns
    ///
    /// Returns the output when the controller's work is done, or an error
    /// if something goes wrong.
    fn run(&mut self) -> impl Future<Output = Result<O, Error>> + Send;
}

/// Manages a sequence of controllers.
///
/// The Coordinator runs controllers in sequence, determining the next
/// controller based on the output of the current one. This prevents
/// stack overflow from recursive controller calls.
///
/// This pattern is based on the Coordinator pattern from MVVM-C architecture.
pub struct Coordinator;

impl Coordinator {
    /// Runs controllers in a sequence until completion.
    ///
    /// This method runs the initial controller, then uses the provided
    /// closure to determine the next controller based on the output.
    /// The loop continues until the closure returns `None`.
    ///
    /// # Type Parameters
    ///
    /// - `C`: The controller type (must implement [`Controller<O>`])
    /// - `O`: The output type from the controller
    /// - `F`: Closure that determines the next controller
    ///
    /// # Parameters
    ///
    /// - `initial`: The first controller to run
    /// - `next`: Closure that receives the output and returns `Some(controller)`
    ///   to continue, or `None` to exit
    ///
    /// # Returns
    ///
    /// Returns the final output when the sequence completes, or an error
    /// if any controller fails.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Coordinator::run(
    ///     SettingsMainController::new(&ctx),
    ///     |result| match result {
    ///         MainResult::NavigateToFeeds => Some(SettingsFeedController::new(&ctx)),
    ///         MainResult::NavigateToVoice => Some(SettingsVoiceController::new(&ctx)),
    ///         MainResult::Exit => None,
    ///     }
    /// ).await?;
    /// ```
    pub async fn run<C, O, F>(mut current: C, mut next: F) -> Result<O, Error>
    where
        C: Controller<O>,
        F: FnMut(&O) -> Option<C>,
    {
        loop {
            let output = current.run().await?;
            match next(&output) {
                Some(controller) => current = controller,
                None => return Ok(output),
            }
        }
    }
}
