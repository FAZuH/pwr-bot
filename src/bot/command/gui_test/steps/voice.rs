//! Test steps for voice commands.

use crate::bot::command::prelude::*;
use crate::bot::command::voice::GuildStatType;
use crate::bot::command::voice::VoiceStatsTimeRange;
use crate::bot::command::voice::leaderboard::LEADERBOARD_PER_PAGE;
use crate::bot::command::voice::leaderboard::VoiceLeaderboardHandler;
use crate::bot::command::voice::stats::VoiceStatsData;
use crate::bot::command::voice::stats::VoiceStatsHandler;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_eq_cmd;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::helpers::extract_actions;
use crate::bot::test_framework::helpers::simulate_click;
use crate::bot::view::ViewCmd;
use crate::update::voice_leaderboard::VoiceLeaderboardModel;

pub async fn test_voice_leaderboard(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let guild_id = ctx.guild_id().ok_or(GuiTestError::assertion_failed(
        "voice_leaderboard",
        "guild context",
        "none",
    ))?;
    let author_id = ctx.author().id.get();

    let model = VoiceLeaderboardModel::from_entries(vec![], author_id, LEADERBOARD_PER_PAGE);
    let mut handler = VoiceLeaderboardHandler::new(model, &ctx, guild_id.get(), author_id);

    let registry = extract_actions(&handler);
    let toggle_action = assert_has_action(&registry, "ToggleMode")
        .map_err(|e| GuiTestError::execution_failed("voice_leaderboard render", e))?;
    let coordinator = Coordinator::new(ctx);
    let cmd = simulate_click(ctx, &mut handler, toggle_action, coordinator.clone())
        .await
        .map_err(|e| GuiTestError::execution_failed("voice_leaderboard toggle", e))?;
    assert_eq_cmd(cmd, ViewCmd::Render, "voice_leaderboard toggle")
        .map_err(|e| GuiTestError::execution_failed("voice_leaderboard toggle", e))?;

    Ok(())
}

pub async fn test_voice_stats(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let guild_id = ctx.guild_id().ok_or(GuiTestError::assertion_failed(
        "voice_stats",
        "guild context",
        "none",
    ))?;

    let data = VoiceStatsData {
        user: None,
        guild_name: "Test Server".to_string(),
        user_activity: vec![],
        guild_stats: vec![],
        stat_type: GuildStatType::AverageTime,
        time_range: VoiceStatsTimeRange::Monthly,
        raw_sessions: vec![],
    };

    let mut handler = VoiceStatsHandler::new(
        data,
        ctx.data().service.voice_tracking.clone(),
        guild_id.get(),
        ctx.author().clone(),
    );

    let registry = extract_actions(&handler);
    let toggle_action = assert_has_action(&registry, "ToggleDataMode")
        .map_err(|e| GuiTestError::execution_failed("voice_stats render", e))?;
    let coordinator = Coordinator::new(ctx);
    let cmd = simulate_click(ctx, &mut handler, toggle_action, coordinator.clone())
        .await
        .map_err(|e| GuiTestError::execution_failed("voice_stats toggle", e))?;
    assert_eq_cmd(cmd, ViewCmd::Render, "voice_stats toggle")
        .map_err(|e| GuiTestError::execution_failed("voice_stats toggle", e))?;

    Ok(())
}
