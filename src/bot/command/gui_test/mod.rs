//! Owner-only GUI test suite command.
//!
//! Runs through every interactive slash command, auto-simulating button clicks
//! and select-menu choices to assert view state transitions without blocking on
//! a live [`ViewEngine`] loop.

use std::time::Duration;

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

/// Step status for rendering the live summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepStatus {
    Pending,
    Running,
    Passed,
    Failed,
}

impl StepStatus {
    fn emoji(self) -> &'static str {
        match self {
            StepStatus::Pending => "⬜",
            StepStatus::Running => "⏳",
            StepStatus::Passed => "✅",
            StepStatus::Failed => "❌",
        }
    }
}

/// Builds the Components V2 reply for the current test state.
fn build_test_reply<'a>(
    steps: &'a [(StepStatus, &'static str, &'static str)],
    failed_at: Option<usize>,
) -> CreateReply<'a> {
    let total = steps.len();
    let passed = steps
        .iter()
        .filter(|(s, _, _)| *s == StepStatus::Passed)
        .count();

    let title = if failed_at.is_some() {
        "🔧 GUI Test Suite — FAILED"
    } else if passed == total {
        "🔧 GUI Test Suite — COMPLETE"
    } else {
        "🔧 GUI Test Suite"
    };

    let mut body_lines = vec![];

    for (i, (status, name, desc)) in steps.iter().enumerate() {
        let line = format!("{} **{}** — {}", status.emoji(), name, desc);
        body_lines.push(line);

        // If this step failed, stop listing here
        if let Some(failed_idx) = failed_at
            && i == failed_idx
        {
            break;
        }
    }

    let status_text = if let Some(idx) = failed_at {
        format!("\n❌ **Failed at:** {}\n", steps[idx].1)
    } else if passed == total {
        format!("\n✅ **All {} steps passed.**\n", total)
    } else {
        format!("\n⏳ **Progress:** {}/{}\n", passed, total)
    };

    let text = format!("{}\n{}\n{}", title, body_lines.join("\n"), status_text);

    let container = CreateComponent::Container(CreateContainer::new(vec![
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(text)),
    ]));

    CreateReply::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(vec![container])
}

/// Run the automated GUI test suite
///
/// Owner-only command that validates every interactive view by simulating
/// clicks and asserting state transitions. Stops gracefully on first failure.
#[poise::command(slash_command, owners_only, hide_in_help)]
pub async fn gui_test(ctx: Context<'_>) -> Result<(), Error> {
    let total = TEST_STEPS.len();
    let mut statuses: Vec<StepStatus> = vec![StepStatus::Pending; total];

    // Pre-build the static step info to avoid lifetime issues
    let step_info: Vec<(&'static str, &'static str)> =
        TEST_STEPS.iter().map(|s| (s.name, s.description)).collect();

    // Send initial message
    let steps_with_status: Vec<_> = statuses
        .iter()
        .zip(step_info.iter())
        .map(|(s, (name, desc))| (*s, *name, *desc))
        .collect();
    let msg = ctx.send(build_test_reply(&steps_with_status, None)).await?;

    for (i, step) in TEST_STEPS.iter().enumerate() {
        statuses[i] = StepStatus::Running;

        let steps_with_status: Vec<_> = statuses
            .iter()
            .zip(step_info.iter())
            .map(|(s, (name, desc))| (*s, *name, *desc))
            .collect();
        msg.edit(ctx, build_test_reply(&steps_with_status, None))
            .await?;

        match (step.run)(ctx).await {
            Ok(()) => {
                statuses[i] = StepStatus::Passed;
            }
            Err(e) => {
                statuses[i] = StepStatus::Failed;

                let steps_with_status: Vec<_> = statuses
                    .iter()
                    .zip(step_info.iter())
                    .map(|(s, (name, desc))| (*s, *name, *desc))
                    .collect();
                msg.edit(
                    ctx,
                    build_test_reply(&steps_with_status, Some(i))
                        .content(format!("```\n{}\n```", e)),
                )
                .await?;
                return Ok(());
            }
        }

        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    // Final complete state
    let steps_with_status: Vec<_> = statuses
        .iter()
        .zip(step_info.iter())
        .map(|(s, (name, desc))| (*s, *name, *desc))
        .collect();
    msg.edit(ctx, build_test_reply(&steps_with_status, None))
        .await?;

    Ok(())
}
