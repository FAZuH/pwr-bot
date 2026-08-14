//! Canonical test-plugin fixture for the pwr-bot plugin wire protocol.
//!
//! JSON-Lines over stdio: one compact JSON object per line, terminated by a
//! single `\n`. stdout carries ONLY protocol lines; stderr is the free logging
//! channel. Every line written to stdout must be followed by an explicit
//! flush: piped stdout is block-buffered, and a missed flush deadlocks the
//! host waiting for a reply (the bug that killed the prototype).
//!
//! Protocol behavior:
//! - announces `hello` (`v`, `name`, `caps`) as its first line after spawn;
//! - answers `call` (`invoke`, `view.interact`) with a correlation-id-matched
//!   `resp`, keeping a per-process click counter for [`BUTTON_CUSTOM_ID`];
//! - treats `event` (e.g. `view.timeout`) as one-way, never answering it;
//! - answers `ping` with `pong`;
//! - tolerates the host's hello ack silently;
//! - exits 0 on `bye` and on EOF.
//!
//! Drive it from integration tests via `CARGO_BIN_EXE_hello_plugin` (see
//! `tests/plugin_hello_world.rs`).

use std::io::BufRead;
use std::io::Write;
use std::process::ExitCode;

use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::BUTTON_CUSTOM_ID;
use pwr_plugin_protocol::CommandDef;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::PLUGIN_NAME;
use pwr_plugin_protocol::WireError;
use serde_json::Value;
use serde_json::json;

/// The view payload the fixture renders: raw Discord message JSON with one
/// action-row button carrying the [`BUTTON_CUSTOM_ID`] custom id.
fn view_data(content: &str) -> Value {
    json!({
        "content": content,
        "components": [{
            "type": 1,
            "components": [{
                "type": 2,
                "custom_id": BUTTON_CUSTOM_ID,
                "label": "Click me",
                "style": 1
            }]
        }],
        "flags": 0
    })
}

/// The fixture's static declaration, matching what its hello announces. The
/// plugin refuses to start if the host's [`Manifest::validate`] rejects it.
fn manifest() -> Manifest {
    Manifest {
        name: PLUGIN_NAME.into(),
        description: "Canonical test plugin".into(),
        version: "0.1.0".into(),
        commands: vec![CommandDef {
            create_command: json!({"name": PLUGIN_NAME, "description": "Say hello from a plugin"}),
        }],
        event_handlers: vec!["view.timeout".into()],
        tasks: vec![],
        settings_panels: vec![],
        api_version: API_VERSION,
    }
}

/// Serializes `msg` to one JSON line, writes it, then flushes. Every protocol
/// line must end with `\n` and be flushed before the host can read it — piped
/// stdout is block-buffered, unlike the test harness's `send_line`.
fn write_msg(out: &mut impl Write, msg: &Msg) -> std::io::Result<()> {
    let line = serde_json::to_string(msg).expect("serialize protocol message");
    writeln!(out, "{line}")?;
    out.flush()
}

fn main() -> ExitCode {
    if let Err(e) = manifest().validate() {
        // stderr is the designated logging channel (no `log` dep in this
        // binary) — intentional deviation from AGENTS.md's log-macro convention.
        eprintln!("manifest invalid: {e}");
        return ExitCode::FAILURE;
    }

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut count: u64 = 0;

    // Announce ourselves: the plugin, not the host, sends hello first.
    let hello = Msg::Hello {
        v: API_VERSION,
        name: PLUGIN_NAME.into(),
        caps: vec!["command:hello".into()],
    };
    if write_msg(&mut out, &hello).is_err() {
        return ExitCode::FAILURE;
    }

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break }; // EOF => clean exit
        let msg: Msg = match serde_json::from_str(&line) {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("bad json: {e}");
                continue;
            }
        };
        match msg {
            Msg::Bye => break,
            Msg::Call { id, op, cmd, args } => {
                let is_click = args
                    .as_ref()
                    .and_then(|a| a.get("custom_id"))
                    .and_then(Value::as_str)
                    == Some(BUTTON_CUSTOM_ID);
                let resp = match (op.as_str(), cmd.as_deref(), is_click) {
                    ("invoke", Some(PLUGIN_NAME), _) => {
                        Msg::resp_ok(id, Some(view_data("Hello from plugin!")))
                    }
                    ("view.interact", Some(PLUGIN_NAME), true) => {
                        count += 1;
                        let content = format!("Button clicked! count={count}");
                        Msg::resp_ok(id, Some(view_data(&content)))
                    }
                    ("view.interact", Some(PLUGIN_NAME), false) => Msg::resp_err(
                        id,
                        WireError {
                            kind: "UnknownAction".into(),
                            msg: "unknown custom_id".into(),
                        },
                    ),
                    _ => {
                        let cmd_repr = cmd.as_deref().unwrap_or("");
                        Msg::resp_err(
                            id,
                            WireError {
                                kind: "UnknownOp".into(),
                                msg: format!("unknown op {op} for cmd {cmd_repr}"),
                            },
                        )
                    }
                };
                if write_msg(&mut out, &resp).is_err() {
                    return ExitCode::FAILURE;
                }
            }
            Msg::Event { name, .. } => {
                // One-way push: never reply. Note it on stderr only.
                if name == "view.timeout" {
                    eprintln!("event: view.timeout received");
                }
            }
            Msg::Ping => {
                if write_msg(&mut out, &Msg::Pong).is_err() {
                    return ExitCode::FAILURE;
                }
            }
            Msg::Pong => {}
            // The host answers our hello with its own; tolerate it silently.
            // Logging it would be noise, and the stderr test asserts on a
            // dedicated fixture line instead.
            Msg::Hello { .. } => {}
            Msg::Resp { .. } => eprintln!("unexpected message: {line}"),
        }
    }
    ExitCode::SUCCESS
}
