//! Voice leaderboard subcommand — delegates to the voice plugin.

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::bot::command::voice::VoiceLeaderboardTimeRange;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_builtin;

pub mod image_builder;
pub mod image_generator;

/// Display the voice activity leaderboard
///
/// Shows a ranked list of users by total time spent in voice channels.
#[poise::command(slash_command)]
pub async fn leaderboard(
    ctx: Context<'_>,
    #[description = "Time period to filter voice activity. Defaults to \"This month\""]
    _time_range: Option<VoiceLeaderboardTimeRange>,
) -> Result<(), Error> {
    let host_ctx = Arc::new(PoiseHostCtx::new(ctx));
    let plugin = pwr_bot_plugin_voice::VoicePlugin::new();
    let args = serde_json::json!({"action": "leaderboard"});
    dispatch_builtin(&host_ctx, &plugin, "vc leaderboard", args).await
}
