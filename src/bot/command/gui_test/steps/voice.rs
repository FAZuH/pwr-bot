//! Test steps for voice commands.
//!
//! Validates the plugin-based voice commands via `dispatch_builtin`.

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_builtin;
use crate::bot::test_framework::GuiTestError;

pub async fn voice_leaderboard(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let host_ctx = Arc::new(PoiseHostCtx::new(ctx));
    let plugin = pwr_bot_plugin_voice::VoicePlugin::new();
    let args = serde_json::json!({"action": "leaderboard"});
    dispatch_builtin(&host_ctx, &plugin, "vc leaderboard", args)
        .await
        .map_err(|e| GuiTestError::execution_failed("voice_leaderboard", e.to_string()))?;
    Ok(())
}

pub async fn voice_stats(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let host_ctx = Arc::new(PoiseHostCtx::new(ctx));
    let plugin = pwr_bot_plugin_voice::VoicePlugin::new();
    let args = serde_json::json!({"action": "stats"});
    dispatch_builtin(&host_ctx, &plugin, "vc stats", args)
        .await
        .map_err(|e| GuiTestError::execution_failed("voice_stats", e.to_string()))?;
    Ok(())
}
