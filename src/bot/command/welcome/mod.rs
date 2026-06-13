//! Welcome command — delegates to the welcome plugin via FFI-style dispatch.

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_plugin_command;

/// Manage welcome message settings via the welcome plugin.
#[poise::command(slash_command)]
pub async fn welcome(ctx: Context<'_>) -> Result<(), Error> {
    let host_ctx = Arc::new(PoiseHostCtx::new(ctx));
    let args = serde_json::json!({});
    dispatch_plugin_command(&ctx.data().plugin_registry, &host_ctx, "welcome", args).await
}
