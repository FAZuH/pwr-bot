//! Generic test step that runs all dynamically discovered plugin test steps.
//!
//! Iterates over `TestStepSpec`s declared by loaded plugins and dispatches
//! each one via `dispatch_plugin_command`. Fails on the first error.

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_plugin_command;
use crate::bot::test_framework::GuiTestError;

pub async fn plugin_tests(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let registry = &ctx.data().plugin_registry;
    let steps = registry.all_test_steps().await;

    if steps.is_empty() {
        return Ok(());
    }

    for (_plugin_name, _step_name, spec) in &steps {
        let host_ctx = Arc::new(PoiseHostCtx::new(ctx));
        dispatch_plugin_command(registry, &host_ctx, &spec.command, spec.args.clone())
            .await
            .map_err(|e| GuiTestError::execution_failed(&spec.name, e.to_string()))?;
    }

    Ok(())
}
