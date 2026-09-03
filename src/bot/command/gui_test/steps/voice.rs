//! Test steps for voice commands.

use crate::bot::command::prelude::*;
use crate::bot::command::voice::GuildStatType;
use crate::bot::command::voice::VoiceStatsTimeRange;
use crate::bot::gui::voice_leaderboard::LEADERBOARD_PER_PAGE;
use crate::bot::gui::voice_leaderboard::VoiceLeaderboardFeature;
use crate::bot::gui::voice_stats::VoiceStatsFeature;
use crate::bot::test_framework::GuiTestError;
use crate::bot::test_framework::assert::assert_has_action;
use crate::bot::test_framework::helpers::apply_feature_msg;
use crate::bot::test_framework::helpers::feature_actions;
use crate::bot::test_framework::helpers::translate_feature_action;
use crate::update::voice_leaderboard::VoiceLeaderboardEffect;
use crate::update::voice_leaderboard::VoiceLeaderboardModel;
use crate::update::voice_stats::VoiceStatsData;
use crate::update::voice_stats::VoiceStatsEffect;
use crate::update::voice_stats::VoiceStatsModel;

pub async fn voice_leaderboard(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let _guild_id = ctx.guild_id().ok_or(GuiTestError::assertion_failed(
        "voice_leaderboard",
        "guild context",
        "none",
    ))?;
    let author_id = ctx.author().id.get();

    let mut model = VoiceLeaderboardModel::from_entries(vec![], author_id, LEADERBOARD_PER_PAGE);

    let registry = feature_actions::<VoiceLeaderboardFeature>(&model);
    let toggle_action = assert_has_action(&registry, "ToggleMode")
        .map_err(|e| GuiTestError::execution_failed("voice_leaderboard render", e))?;

    let msg = translate_feature_action::<VoiceLeaderboardFeature>(&toggle_action, &model)
        .ok_or_else(|| {
            GuiTestError::execution_failed(
                "voice_leaderboard toggle",
                "action did not translate to a message",
            )
        })?;
    let effects = apply_feature_msg::<VoiceLeaderboardFeature>(msg, &mut model);

    if !effects
        .iter()
        .any(|e| matches!(e, VoiceLeaderboardEffect::QueryLeaderboard { .. }))
    {
        return Err(GuiTestError::execution_failed(
            "voice_leaderboard toggle",
            "expected a QueryLeaderboard effect",
        ));
    }
    if !model.is_partner_mode() {
        return Err(GuiTestError::assertion_failed(
            "voice_leaderboard toggle",
            true,
            model.is_partner_mode(),
        ));
    }

    Ok(())
}

pub async fn voice_stats(ctx: Context<'_>) -> Result<(), GuiTestError> {
    let _guild_id = ctx.guild_id().ok_or(GuiTestError::assertion_failed(
        "voice_stats",
        "guild context",
        "none",
    ))?;

    let data = VoiceStatsData {
        guild_name: "Test Server".to_string(),
        user_activity: vec![],
        guild_stats: vec![],
        raw_sessions: vec![],
        target_user_name: None,
    };

    let mut model = VoiceStatsModel::new(
        VoiceStatsTimeRange::Monthly,
        GuildStatType::AverageTime,
        None,
        1,
        data,
        None,
    );

    let registry = feature_actions::<VoiceStatsFeature>(&model);
    let toggle_action = assert_has_action(&registry, "ToggleDataMode")
        .map_err(|e| GuiTestError::execution_failed("voice_stats render", e))?;

    let msg =
        translate_feature_action::<VoiceStatsFeature>(&toggle_action, &model).ok_or_else(|| {
            GuiTestError::execution_failed(
                "voice_stats toggle",
                "action did not translate to a message",
            )
        })?;
    let effects = apply_feature_msg::<VoiceStatsFeature>(msg, &mut model);

    if !effects
        .iter()
        .any(|e| matches!(e, VoiceStatsEffect::QueryStats { .. }))
    {
        return Err(GuiTestError::execution_failed(
            "voice_stats toggle",
            "expected a QueryStats effect",
        ));
    }
    if !model.is_user_stats() {
        return Err(GuiTestError::assertion_failed(
            "voice_stats toggle",
            true,
            model.is_user_stats(),
        ));
    }

    Ok(())
}
