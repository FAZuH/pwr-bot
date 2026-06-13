//! Test step for the `/feed list` command.
//!
//! Validates the plugin-based feed list command by calling `dispatch_builtin`.

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_builtin;
use crate::bot::test_framework::GuiTestError;

pub async fn feed_list_empty(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let host_ctx = Arc::new(PoiseHostCtx::new(ctx));
    let plugin = pwr_bot_plugin_feed::FeedPlugin::new();
    let args = serde_json::json!({"action": "list"});
    dispatch_builtin(&host_ctx, &plugin, "feed list", args)
        .await
        .map_err(|e| GuiTestError::execution_failed("feed_list_empty", e.to_string()))?;

    Ok(())
}
