//! Integration tests for dispatch's argument re-parse: a real
//! `arg_echo_plugin` fixture subprocess echoes back the args of every
//! `invoke` call, so these tests assert that the args a dispatch would
//! produce actually reach the plugin over the wire. Pure stdio — no database.
//!
//! The canonical `hello_plugin` renders a static view and cannot echo args,
//! and the protocol crate is frozen, so this fixture lives in the host crate
//! (`src/bin/arg_echo_plugin.rs`) and is built with the test binary.

use std::path::PathBuf;
use std::sync::Arc;

use poise::serenity_prelude as serenity;
use pwr_bot::plugin::InteractionEngine;
use pwr_bot::plugin::RunningPlugin;
use pwr_bot::plugin::command::command_from_blob;
use pwr_bot::plugin::command::reparse_command_args;
use pwr_plugin_protocol::ViewSpec;
use serde_json::Value;
use serde_json::json;

/// Locates the `arg_echo_plugin` fixture binary. `CARGO_BIN_EXE_arg_echo_plugin`
/// is set by cargo for the same-package integration tests; fall back to
/// probing `target/{profile}` for manual runs, mirroring `plugin_interaction`.
/// A missing binary panics with a build hint rather than a confusing spawn
/// error.
fn fixture_path() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_arg_echo_plugin") {
        return PathBuf::from(path);
    }
    let target = match option_env!("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"),
    };
    for profile in ["debug", "release"] {
        let candidate = target.join(profile).join("arg_echo_plugin");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!(concat!(
        "test-plugin fixture not built; run `cargo build -p pwr-bot ",
        "--bin arg_echo_plugin` first"
    ));
}

/// Builds a `CommandInteraction` from a partial payload, the same way serenity
/// does when an interaction arrives; `data` is the command data the
/// interaction carries. Goes through a JSON string, not `from_value`: resolved
/// channels deserialize via `&RawValue`, which only serde_json's string
/// deserializer supports.
fn command_interaction(data: Value) -> serenity::CommandInteraction {
    serde_json::from_str(
        &json!({
            "id": "1",
            "application_id": "1",
            "channel_id": "1",
            "token": "token",
            "version": 1,
            "app_permissions": "0",
            "locale": "en-US",
            "entitlements": [],
            "attachment_size_limit": 0,
            "data": data,
        })
        .to_string(),
    )
    .expect("command interaction deserializes")
}

/// Extracts the echoed args JSON from the fixture's resp envelope.
fn parse_echoed_args(spec: &ViewSpec) -> Value {
    serde_json::from_str(spec.data["content"].as_str().expect("echoed args string"))
        .expect("echoed args parse")
}

/// Echoes `args` through the fixture: spawns it and opens an engine session
/// with `args`, returning the plugin (owned by the caller, stopped at the end
/// of the test) and the args the fixture echoed back.
async fn echo_args(command: &str, args: Value) -> (Arc<RunningPlugin>, Value) {
    let plugin = Arc::new(
        RunningPlugin::spawn(fixture_path())
            .await
            .expect("spawn arg_echo_plugin"),
    );
    let engine = InteractionEngine::new();
    let spec = engine
        .open(serenity::MessageId::new(1), plugin.clone(), command, args)
        .await
        .expect("open the view");
    let echoed = parse_echoed_args(&spec);
    (plugin, echoed)
}

// ── parsed args reach the plugin over the wire ─────────────────────────────

#[tokio::test]
async fn parsed_args_reach_the_plugin_over_the_wire() {
    let command = command_from_blob(&json!({
        "name": "echo",
        "description": "Echo",
        "options": [
            {"name": "text", "description": "Text", "type": 3, "required": true},
            {"name": "count", "description": "Count", "type": 4},
        ],
    }))
    .expect("command from blob");
    let interaction = command_interaction(json!({
        "id": "1",
        "name": "echo",
        "type": 1,
        "options": [
            {"name": "text", "type": 3, "value": "hi"},
            {"name": "count", "type": 4, "value": 42},
        ],
    }));

    let args = reparse_command_args(&command, &interaction).expect("args reparse");
    let (plugin, echoed) = echo_args("echo", args).await;

    // serde_json maps are BTreeMap-backed: compare parsed values, not strings.
    assert_eq!(echoed, json!({"text": "hi", "count": 42}));

    plugin.stop().await.expect("graceful stop");
}

#[tokio::test]
async fn subcommand_leaf_args_reach_the_plugin() {
    let command = command_from_blob(&json!({
        "name": "job",
        "description": "Job",
        "options": [{
            "name": "run",
            "description": "Run the job",
            "type": 1,
            "options": [{"name": "url", "description": "URL", "type": 3, "required": true}],
        }],
    }))
    .expect("command from blob");
    let run = &command.subcommands[0];
    let interaction = command_interaction(json!({
        "id": "1",
        "name": "job",
        "type": 1,
        "options": [{
            "name": "run",
            "type": 1,
            "options": [{"name": "url", "type": 3, "value": "https://example.com"}],
        }],
    }));

    let args = reparse_command_args(run, &interaction).expect("leaf args reparse");
    let (plugin, echoed) = echo_args("run", args).await;

    assert_eq!(echoed, json!({"url": "https://example.com"}));

    plugin.stop().await.expect("graceful stop");
}
