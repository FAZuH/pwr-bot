//! The synchronous plugin wire session: spawn, hello handshake, invoke,
//! shutdown.
//!
//! Framing uses [`pwr_plugin_protocol`]'s own types throughout (`Msg`,
//! `Manifest`) — no wire parsing is reimplemented and nothing is imported
//! from the host crate. Everything is blocking std stdio: this tool makes
//! one call per run, so no async runtime is warranted.

use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::process::Child;
use std::process::ChildStdin;
use std::process::ChildStdout;
use std::process::Command;
use std::process::Stdio;

use anyhow::Context;
use anyhow::Result;
use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::Manifest;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::WireError;
use pwr_plugin_protocol::validate_caps;
use serde_json::Value;

/// A plugin subprocess speaking the wire protocol over stdio, from the
/// preview tool's side — a minimal host.
pub struct PluginSession {
    /// The child handle; killed on drop if the session never shut down.
    child: Child,
    /// Writer to the plugin's stdin. `None` after [`PluginSession::shutdown`]
    /// — dropping the pipe closes it, and the plugin treats stdin EOF as
    /// exit.
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    /// Host-side monotonic call-id source.
    next_id: u64,
    /// The plugin's announced name.
    pub name: String,
    /// The plugin's validated manifest, when its hello carried one.
    pub manifest: Option<Manifest>,
}

impl PluginSession {
    /// Spawns the plugin at `path` and runs the hello handshake: the plugin
    /// announces first, then this side validates the version, the `host.*`
    /// caps, and the manifest (all through `pwr_plugin_protocol`, mirroring
    /// the host's spawn gate in `src/plugin/mod.rs`) and acks with its own
    /// hello. The plugin's stderr is inherited, keeping its free logging
    /// channel visible in the terminal.
    pub fn spawn(path: &Path) -> Result<PluginSession> {
        let label = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("plugin")
            .to_owned();

        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("spawning the plugin at `{}`", path.display()))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));

        let mut session = PluginSession {
            child,
            stdin: Some(stdin),
            stdout,
            next_id: 0,
            name: label.clone(),
            manifest: None,
        };

        let hello = session
            .read_message()
            .with_context(|| format!("reading the `{label}` plugin's hello"))?;
        let Msg::Hello {
            v,
            name,
            caps,
            manifest,
        } = hello
        else {
            anyhow::bail!("the `{label}` plugin's first message was not a hello");
        };
        if v != API_VERSION {
            anyhow::bail!(
                "the `{name}` plugin speaks protocol version {v}; this tool speaks {API_VERSION}"
            );
        }
        validate_caps(&caps).with_context(|| format!("validating the `{name}` plugin's caps"))?;
        if let Some(manifest) = &manifest {
            manifest
                .validate()
                .with_context(|| format!("validating the `{name}` plugin's manifest"))?;
            if manifest.name != name {
                anyhow::bail!(
                    "the `{name}` plugin's manifest names itself `{}`",
                    manifest.name
                );
            }
        }
        session.name = name;
        session.manifest = manifest;

        // Acknowledge with this side's hello: the preview serves no host
        // ops, so it announces no caps — the honest handshake, and the
        // plugins tolerate the ack silently either way.
        let ack = Msg::Hello {
            v: API_VERSION,
            name: "preview".into(),
            caps: Vec::new(),
            manifest: None,
        };
        session
            .write_message(&ack)
            .with_context(|| format!("acking the `{}` plugin's hello", session.name))?;

        Ok(session)
    }

    /// Invokes the plugin's `command` and returns the successful resp's data
    /// payload. Plugin→host calls interleaved with the reply are answered
    /// `HostUnavailable` (this side serves no host ops, exactly like a
    /// service-less host spawn); plugin→host events and pings are handled
    /// per the protocol so the reply can arrive.
    pub fn invoke(&mut self, command: &str, args: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let call = Msg::Call {
            id,
            op: "invoke".into(),
            cmd: Some(command.to_owned()),
            args: Some(args),
        };
        self.write_message(&call)
            .with_context(|| format!("sending the `{command}` invoke"))?;
        loop {
            let message = self
                .read_message()
                .with_context(|| format!("waiting for the `{command}` invoke reply"))?;
            match message {
                Msg::Resp {
                    id: reply_id,
                    ok: true,
                    data: Some(data),
                    ..
                } if reply_id == id => return Ok(data),
                Msg::Resp {
                    id: reply_id,
                    ok: true,
                    data: None,
                    ..
                } if reply_id == id => anyhow::bail!(
                    "the `{}` plugin answered the `{command}` invoke without a payload",
                    self.name
                ),
                Msg::Resp {
                    id: reply_id,
                    ok: false,
                    error,
                    ..
                } if reply_id == id => {
                    let error = error.unwrap_or_else(|| WireError {
                        kind: "Unknown".into(),
                        msg: "ok:false without an error".into(),
                    });
                    anyhow::bail!(
                        "the `{}` plugin rejected the `{command}` invoke: {} ({})",
                        self.name,
                        error.kind,
                        error.msg
                    );
                }
                // Some other id's resp: unexpected in this single-call
                // session; note it and keep waiting for ours.
                Msg::Resp { id: reply_id, .. } => {
                    eprintln!("note: plugin resp for unknown id {reply_id} ignored");
                }
                Msg::Call {
                    id: reply_id, op, ..
                } => {
                    let rejected = Msg::resp_err(
                        reply_id,
                        WireError {
                            kind: "HostUnavailable".into(),
                            msg: format!("preview serves no host ops (`{op}`)"),
                        },
                    );
                    self.write_message(&rejected)
                        .with_context(|| format!("answering the plugin's `{op}` call"))?;
                }
                Msg::Event { name, .. } => {
                    eprintln!("plugin event: {name}");
                }
                Msg::Ping => {
                    self.write_message(&Msg::Pong)
                        .with_context(|| "answering the plugin's ping")?;
                }
                Msg::Pong => {}
                Msg::Hello { .. } => {
                    eprintln!("note: plugin sent a second hello; ignored");
                }
                Msg::Bye => {
                    anyhow::bail!(
                        "the `{}` plugin sent bye before answering the `{command}` invoke",
                        self.name
                    );
                }
            }
        }
    }

    /// Gracefully shuts the plugin down: sends `bye`, closes stdin (EOF),
    /// and waits for the exit. A nonzero exit is reported as a warning on
    /// stderr, not an error — the run already has what it came for.
    pub fn shutdown(mut self) -> Result<()> {
        let name = self.name.clone();
        if let Some(mut stdin) = self.stdin.take() {
            if let Err(error) = write_line(&mut stdin, &Msg::Bye) {
                eprintln!("warning: could not send bye to the `{name}` plugin: {error}");
            }
            // Dropping the pipe closes stdin; the plugin exits on EOF even
            // without the bye.
            drop(stdin);
        }
        match self.child.wait() {
            Ok(status) if !status.success() => {
                eprintln!("warning: the `{name}` plugin exited {status} on bye");
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("waiting for the `{name}` plugin to exit"))
            }
        }
    }

    /// Reads one protocol message (a single JSON line). EOF and undecodable
    /// lines are errors — a closed stdout mid-run is the plugin dying.
    fn read_message(&mut self) -> Result<Msg> {
        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .with_context(|| "reading a protocol line from the plugin's stdout")?;
        if read == 0 {
            anyhow::bail!("the plugin closed its stdout");
        }
        serde_json::from_str(line.trim())
            .with_context(|| format!("decoding the plugin's protocol line `{}`", line.trim()))
    }

    /// Writes one protocol message as a compact JSON line, then flushes — a
    /// piped stdin write without a flush would deadlock the handshake.
    /// Fails with a stopped-plugin error once [`PluginSession::shutdown`]
    /// took the stdin pipe.
    fn write_message(&mut self, message: &Msg) -> Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .with_context(|| "the plugin's stdin is already closed")?;
        write_line(stdin, message)
    }
}

/// Writes one protocol message to the plugin's stdin as a compact JSON line,
/// then flushes. Every protocol line ends with `\n` and is flushed before
/// the plugin can read it — a piped stdin write without a flush would
/// deadlock the handshake.
fn write_line(stdin: &mut ChildStdin, message: &Msg) -> Result<()> {
    let line = serde_json::to_string(message).expect("serialize protocol message");
    writeln!(stdin, "{line}")
        .and_then(|()| stdin.flush())
        .with_context(|| "writing a protocol line to the plugin's stdin")
}

impl Drop for PluginSession {
    /// Kills an un-shut-down plugin so a failed run never orphans it. On the
    /// happy path `shutdown` already waited, and killing a reaped child
    /// errors harmlessly.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Extracts the Discord message JSON from an invoke resp payload. Plugins
/// answer in two shapes, both accepted (mirroring the host's
/// `view_spec_from_data` in `src/plugin/interaction.rs`, which is host code
/// and not importable here):
///
/// - the full envelope `{"data": …, "ephemeral": …, "view": …}` — its
///   `data` field is the message (Discord messages have no top-level `data`
///   field, so the key check is unambiguous);
/// - the v1 raw shape: the Discord message JSON itself.
pub fn message_from_resp_data(data: &Value) -> Value {
    match data {
        Value::Object(map) if map.contains_key("data") => {
            map.get("data").cloned().unwrap_or_default()
        }
        _ => data.clone(),
    }
}

/// The command an invoke should target by default: the manifest's single
/// declared command name. `None` when the manifest is absent or declares
/// anything other than exactly one named command — the caller then needs an
/// explicit `--command`.
pub fn single_command_name(manifest: Option<&Manifest>) -> Option<String> {
    let manifest = manifest?;
    if manifest.commands.len() != 1 {
        return None;
    }
    manifest
        .commands
        .first()
        .and_then(|command| command.create_command.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A valid manifest whose `commands` list holds `command_count` identical
    /// command blobs (each named "hello").
    fn test_manifest(command_count: usize) -> Manifest {
        Manifest {
            name: "hello".into(),
            description: "Canonical test plugin".into(),
            version: "0.1.0".into(),
            commands: std::iter::repeat_n(
                pwr_plugin_protocol::CommandDef {
                    create_command: json!({"name": "hello", "description": "Say hello"}),
                },
                command_count,
            )
            .collect(),
            event_handlers: vec![],
            tasks: vec![],
            api_version: API_VERSION,
        }
    }

    #[test]
    fn envelope_payloads_yield_their_data_field() {
        let payload = json!({
            "data": {"content": "Hello from plugin!", "components": []},
            "ephemeral": false,
            "view": {"page": 1},
        });
        assert_eq!(
            message_from_resp_data(&payload),
            json!({"content": "Hello from plugin!", "components": []})
        );
    }

    #[test]
    fn envelope_payloads_pass_a_null_data_field_through() {
        let payload = json!({"data": null, "ephemeral": true, "view": {}});
        assert_eq!(message_from_resp_data(&payload), Value::Null);
    }

    #[test]
    fn raw_message_payloads_pass_through_verbatim() {
        let payload = json!({"content": "hi", "components": [{"type": 1}]});
        assert_eq!(message_from_resp_data(&payload), payload);
    }

    #[test]
    fn raw_messages_with_view_fields_but_no_data_key_pass_through() {
        // A raw message could carry arbitrary unknown fields; only the
        // top-level `data` key switches to envelope interpretation.
        let payload = json!({"content": "hi", "view": {"page": 1}});
        assert_eq!(message_from_resp_data(&payload), payload);
    }

    #[test]
    fn non_object_payloads_pass_through_verbatim() {
        let payload = json!("just a string");
        assert_eq!(message_from_resp_data(&payload), payload);
    }

    #[test]
    fn single_command_name_returns_the_only_declared_command() {
        assert_eq!(
            single_command_name(Some(&test_manifest(1))),
            Some("hello".into())
        );
    }

    #[test]
    fn single_command_name_is_none_without_a_manifest() {
        assert_eq!(single_command_name(None), None);
    }

    #[test]
    fn single_command_name_is_none_for_zero_or_multiple_commands() {
        assert_eq!(single_command_name(Some(&test_manifest(0))), None);
        assert_eq!(single_command_name(Some(&test_manifest(2))), None);
    }

    #[test]
    fn single_command_name_is_none_when_the_blob_has_no_string_name() {
        let mut malformed = test_manifest(1);
        malformed.commands[0] = pwr_plugin_protocol::CommandDef {
            create_command: json!({"description": "no name"}),
        };
        assert_eq!(single_command_name(Some(&malformed)), None);
    }
}
