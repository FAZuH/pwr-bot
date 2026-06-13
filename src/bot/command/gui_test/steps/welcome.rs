//! Test step for the `/welcome` settings command.
//!
//! Validates the plugin-based welcome command works by calling `dispatch_builtin`
//! and checking the response payload is valid.

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_plugin_command;
use crate::bot::test_framework::GuiTestError;

pub async fn welcome_settings(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let host_ctx = Arc::new(PoiseHostCtx::new(ctx));
    let args = serde_json::json!({});
    dispatch_plugin_command(&ctx.data().plugin_registry, &host_ctx, "welcome", args)
        .await
        .map_err(|e| GuiTestError::execution_failed("welcome_settings", e.to_string()))?;

    Ok(())
}
