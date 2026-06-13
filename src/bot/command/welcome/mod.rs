//! Welcome command — delegates to the welcome plugin via FFI-style dispatch.

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_builtin;

pub mod image_generator;

/// Manage welcome message settings via the welcome plugin.
#[poise::command(slash_command)]
pub async fn welcome(ctx: Context<'_>) -> Result<(), Error> {
    let host_ctx = Arc::new(PoiseHostCtx::new(ctx));
    let plugin = pwr_bot_plugin_welcome::WelcomePlugin;
    let args = serde_json::json!({});
    dispatch_builtin(&host_ctx, &plugin, "welcome", args).await
}
