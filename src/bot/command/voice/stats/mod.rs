//! Voice stats subcommand — delegates to the voice plugin.

use std::sync::Arc;

use crate::bot::command::prelude::*;
use crate::bot::command::voice::GuildStatType;
use crate::bot::command::voice::VoiceStatsTimeRange;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_builtin;

/// Show voice activity statistics
///
/// Display daily voice activity for a user or the entire server.
#[poise::command(slash_command)]
pub async fn stats(
    ctx: Context<'_>,
    #[description = "Time period to display. Defaults to \"This month\""] _time_range: Option<
        VoiceStatsTimeRange,
    >,
    #[description = "User to show stats for (defaults to server stats in server, yourself in DM)"]
    _user: Option<poise::serenity_prelude::User>,
    #[description = "Statistic to display for server view"] _statistic: Option<GuildStatType>,
) -> Result<(), Error> {
    let host_ctx = Arc::new(PoiseHostCtx::new(ctx));
    let plugin = pwr_bot_plugin_voice::VoicePlugin::new();
    let args = serde_json::json!({"action": "stats"});
    dispatch_builtin(&host_ctx, &plugin, "vc stats", args).await
}
