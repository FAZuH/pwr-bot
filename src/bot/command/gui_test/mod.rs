//! Owner-only GUI test suite command.
//!
//! Runs through every interactive slash command, auto-simulating button clicks
//! and select-menu choices to assert view state transitions without blocking on
//! a live [`ViewEngine`] loop.

use std::time::Duration;

use poise::serenity_prelude::CreateEmbed;

use crate::bot::command::prelude::*;
use crate::bot::test_framework::TestStep;

mod steps;

const TEST_STEPS: &[TestStep] = &[
    crate::test_step!(
        "/about",
        "Bot info and statistics",
        steps::about::test_about
    ),
    crate::test_step!(
        "/settings",
        "Main settings page",
        steps::settings::test_settings_main
    ),
    crate::test_step!(
        "/settings > feeds",
        "Feed settings",
        steps::settings::test_feed_settings
    ),
    crate::test_step!(
        "/settings > voice",
        "Voice settings",
        steps::settings::test_voice_settings
    ),
    crate::test_step!(
        "/settings > welcome",
        "Welcome settings",
        steps::welcome::test_welcome_settings
    ),
    crate::test_step!(
        "/feed list",
        "Subscription list (empty)",
        steps::feed::test_feed_list_empty
    ),
    crate::test_step!(
        "/vc leaderboard",
        "Voice leaderboard",
        steps::voice::test_voice_leaderboard
    ),
    crate::test_step!(
        "/vc stats",
        "Voice statistics",
        steps::voice::test_voice_stats
    ),
];

/// Run the automated GUI test suite
///
/// Owner-only command that validates every interactive view by simulating
/// clicks and asserting state transitions. Stops gracefully on first failure.
#[poise::command(slash_command, owners_only, hide_in_help)]
pub async fn gui_test(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .title("🔧 GUI Test Suite")
                .description("Starting test run..."),
        ),
    )
    .await?;

    let total = TEST_STEPS.len();

    for (i, step) in TEST_STEPS.iter().enumerate() {
        let progress = format!("Step {}/{}: {}", i + 1, total, step.name);
        let running_msg = ctx
            .send(
                CreateReply::default().embed(
                    CreateEmbed::new()
                        .title("⏳ Running...")
                        .description(progress.clone())
                        .color(0xFFFF00),
                ),
            )
            .await?;

        match (step.run)(ctx).await {
            Ok(()) => {
                running_msg
                    .edit(
                        ctx,
                        CreateReply::default().embed(
                            CreateEmbed::new()
                                .title("✅ Passed")
                                .description(progress)
                                .color(0x00FF00),
                        ),
                    )
                    .await?;
            }
            Err(e) => {
                ctx.send(
                    CreateReply::default().embed(
                        CreateEmbed::new()
                            .title("❌ FAILED")
                            .description(format!("{}\n\n```\n{}\n```", progress, e))
                            .color(0xFF0000),
                    ),
                )
                .await?;
                return Ok(());
            }
        }

        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    ctx.send(
        CreateReply::default().embed(
            CreateEmbed::new()
                .title("🔧 GUI Test Suite — COMPLETE")
                .description(format!("✅ All {} steps passed.", total))
                .color(0x00FF00),
        ),
    )
    .await?;

    Ok(())
}
