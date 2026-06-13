//! Feed list subcommand — delegates to the feed plugin.

use std::sync::Arc;

use crate::bot::command::feed::SendInto;
use crate::bot::command::prelude::*;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::invocation::dispatch_plugin_command;

/// List your current feed subscriptions
///
/// View all feeds you are subscribed to.
#[poise::command(slash_command)]
pub async fn list(
    ctx: Context<'_>,
    #[description = "Where the notifications are being sent. Default to DM"] _sent_into: Option<
        SendInto,
    >,
) -> Result<(), Error> {
    let host_ctx = Arc::new(PoiseHostCtx::new(ctx));
    let args = serde_json::json!({"action": "list"});
    dispatch_plugin_command(&ctx.data().plugin_registry, &host_ctx, "feed list", args).await
}
