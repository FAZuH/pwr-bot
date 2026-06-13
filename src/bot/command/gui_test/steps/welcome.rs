//! Test step for the `/welcome` settings command.
//!
//! Validates the plugin-based welcome command works by calling `dispatch_builtin`
//! and checking the response payload is valid.

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_builtin;
use crate::bot::test_framework::GuiTestError;

pub async fn welcome_settings(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let host_ctx = Arc::new(PoiseHostCtx::new(ctx));

    let plugin = pwr_bot_plugin_welcome::WelcomePlugin;
    let args = serde_json::json!({});
    dispatch_builtin(&host_ctx, &plugin, "welcome", args)
        .await
        .map_err(|e| GuiTestError::execution_failed("welcome_settings", e.to_string()))?;

    Ok(())
}
