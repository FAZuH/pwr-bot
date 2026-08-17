//! Minimal test-plugin fixture for the host crate's own integration tests:
//! echoes the args of every `invoke` call back in the resp envelope's
//! `data.content`, so a test can assert the parsed arguments a dispatch
//! produced actually reach a plugin over the wire.
//!
//! JSON-Lines over stdio like the canonical `hello_plugin`: one compact JSON
//! object per line, every stdout line flushed before the next read (piped
//! stdout is block-buffered; a missed flush deadlocks the host). The
//! canonical fixture renders a static view, so it cannot serve this
//! assertion; the protocol crate is frozen, so this fixture lives here.

use std::io::BufRead;
use std::io::Write;
use std::process::ExitCode;

use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::CommandDef;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::WireError;
use serde_json::Value;
use serde_json::json;

/// The plugin's hello name and the command it serves.
const PLUGIN_NAME: &str = "arg-echo";

/// Serializes `msg` to one JSON line, writes it, then flushes. Every protocol
/// line must end with `\n` and be flushed before the host can read it.
fn write_msg(out: &mut impl Write, msg: &Msg) -> std::io::Result<()> {
    let line = serde_json::to_string(msg).expect("serialize protocol message");
    writeln!(out, "{line}")?;
    out.flush()
}

fn main() -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // Announce ourselves: the plugin, not the host, sends hello first.
    let hello = Msg::Hello {
        v: API_VERSION,
        name: PLUGIN_NAME.into(),
        caps: vec![format!("command:{PLUGIN_NAME}")],
        manifest: Some(Manifest {
            name: PLUGIN_NAME.into(),
            description: "Echo plugin args".into(),
            version: "0.1.0".into(),
            commands: vec![CommandDef {
                create_command: json!({"name": PLUGIN_NAME, "description": "Echo plugin args"}),
            }],
            event_handlers: vec![],
            tasks: vec![],
            api_version: API_VERSION,
        }),
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
            Msg::Call { id, op, args, .. } if op == "invoke" => {
                let args = args.unwrap_or(Value::Null);
                let resp = Msg::resp_ok(
                    id,
                    Some(json!({
                        "data": {"content": args.to_string()},
                        "ephemeral": false,
                        "view": Value::Null,
                    })),
                );
                if write_msg(&mut out, &resp).is_err() {
                    return ExitCode::FAILURE;
                }
            }
            Msg::Ping => {
                if write_msg(&mut out, &Msg::Pong).is_err() {
                    return ExitCode::FAILURE;
                }
            }
            Msg::Call { id, op, .. } => {
                let resp = Msg::resp_err(
                    id,
                    WireError {
                        kind: "UnknownOp".into(),
                        msg: format!("unknown op {op}"),
                    },
                );
                if write_msg(&mut out, &resp).is_err() {
                    return ExitCode::FAILURE;
                }
            }
            // The host's hello ack and everything else: tolerate silently.
            Msg::Hello { .. } | Msg::Pong | Msg::Event { .. } | Msg::Resp { .. } => {}
        }
    }
    ExitCode::SUCCESS
}
