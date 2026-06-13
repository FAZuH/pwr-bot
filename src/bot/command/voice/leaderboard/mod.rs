//! Voice leaderboard subcommand — delegates to the voice plugin.

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::bot::command::voice::VoiceLeaderboardTimeRange;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_plugin_command;

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
    let args = serde_json::json!({"action": "leaderboard"});
    dispatch_plugin_command(
        &ctx.data().plugin_registry,
        &host_ctx,
        "vc leaderboard",
        args,
    )
    .await
}
