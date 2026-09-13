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
use pwr_plugin_protocol::ViewPayload;
use pwr_plugin_protocol::WireError;
use pwr_plugin_protocol::validate_caps;
use pwr_plugin_protocol::view_payload;
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
/// answer in two shapes, both accepted: the full envelope
/// `{"data": …, "ephemeral": …, "view": …}` (its `data` field is the message)
/// and the v1 raw shape (the Discord message JSON itself). The
/// envelope/raw split is [`view_payload`]'s shared contract — the host
/// applies the same classification in `view_spec_from_data` — and this
/// projection keeps only the message (`data`).
pub fn message_from_resp_data(data: &Value) -> Value {
    match view_payload(data) {
        ViewPayload::Envelope { data, .. } => data,
        ViewPayload::Raw { data } => data,
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
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::Duration;
    use std::time::Instant;

    use pwr_plugin_protocol::API_VERSION;
    use pwr_plugin_protocol::Msg;
    use serde_json::json;
    use tempfile::tempdir;

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

    // ── real-io integration tests (a fake plugin over stdio) ─────────────────

    /// A minimal fake plugin's valid hello: protocol version 1, one command
    /// cap, and a manifest declaring the single `fake` command. It validates
    /// cleanly through [`validate_caps`] and [`pwr_plugin_protocol::Manifest::validate`].
    const FAKE_HELLO: &str = r#"{"t":"hello","v":1,"name":"fake","caps":["command:fake"],"manifest":{"name":"fake","description":"Fake test plugin","version":"0.1.0","commands":[{"create_command":{"name":"fake","description":"Fake"}}],"event_handlers":[],"tasks":[],"api_version":1}}"#;

    /// Writes `body` as an executable shell script at `dir/name` and returns
    /// its path. [`PluginSession::spawn`] execs the path directly, so the
    /// script needs a shebang and the exec bit.
    ///
    /// The write handle is scoped and dropped — flushed and closed — BEFORE
    /// the path is chmod'd or exec'd (Linux refuses to `exec` a file still
    /// open for writing; `ETXTBSY`), so the write-then-close-then-exec order is
    /// strict. The caller keeps `dir` (a `tempdir`) alive for the test, so the
    /// script path survives without holding the file open.
    fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        {
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(body.as_bytes()).unwrap();
            file.flush().unwrap();
        }
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    /// True when `error` is the kernel's transient `ETXTBSY` ("text file
    /// busy") from `execve` of a freshly written script — the source is the
    /// io::Error with raw OS code 26 anywhere in the anyhow chain.
    fn is_etxtbsy(error: &anyhow::Error) -> bool {
        error.chain().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.raw_os_error() == Some(26))
        })
    }

    /// Spawns a fake-plugin script via [`PluginSession::spawn`], retrying a
    /// bounded number of times when the kernel rejects the exec with
    /// `ETXTBSY`. Writing and exec'ing a freshly created executable is subject
    /// to a transient kernel race: even with the write handle closed and the
    /// mode set before exec, `execve` can still report the file as momentarily
    /// busy on a loaded system. `ETXTBSY` is a specific transient OS error, so
    /// a short bounded retry with a small backoff is the deterministic
    /// handling — it cannot mask a plugin/protocol bug, because any real error
    /// (bad version, unknown cap) surfaces immediately without retrying.
    fn spawn_script(path: &Path) -> anyhow::Result<PluginSession> {
        const ATTEMPTS: usize = 8;
        for attempt in 0..ATTEMPTS {
            match PluginSession::spawn(path) {
                Ok(session) => return Ok(session),
                Err(error) if is_etxtbsy(&error) && attempt + 1 < ATTEMPTS => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("the bounded retry loop always returns")
    }

    /// A fake plugin that prints [`FAKE_HELLO`] and then loops reading stdin:
    /// an `invoke` is answered with `{"echo":"pong"}`, a `bye` exits cleanly.
    /// `vars` is emitted before the loop (sidecar paths); `first_read` runs
    /// right after the first stdin line is read but before it is dispatched
    /// (used to capture the host's hello-ack into a sidecar); `before_answer`
    /// runs after the invoke's `id` is parsed but before the answer (an
    /// interleaved host call, or noise); `on_bye` runs when `bye` is read.
    fn fake_plugin(
        dir: &Path,
        name: &str,
        vars: &str,
        first_read: &str,
        before_answer: &str,
        on_bye: &str,
    ) -> PathBuf {
        let mut body = String::from("#!/bin/sh\n");
        body.push_str(vars);
        body.push_str("echo '");
        body.push_str(FAKE_HELLO);
        body.push_str("'\n");
        body.push_str("while IFS= read -r line; do\n");
        body.push_str(first_read);
        body.push_str("  case \"$line\" in\n");
        body.push_str("    *'\"t\":\"bye\"'*)\n");
        body.push_str(on_bye);
        body.push_str("      exit 0 ;;\n");
        body.push_str("    *'\"op\":\"invoke\"'*)\n");
        body.push_str("      id=$(echo \"$line\" | sed -n 's/.*\"id\":\\([0-9]*\\).*/\\1/p')\n");
        body.push_str(before_answer);
        body.push_str("      echo \"{\\\"t\\\":\\\"resp\\\",\\\"id\\\":$id,\\\"ok\\\":true,\\\"data\\\":{\\\"echo\\\":\\\"pong\\\"}}\" ;;\n");
        body.push_str("  esac\n");
        body.push_str("done\n");
        write_script(dir, name, &body)
    }

    /// A fake plugin that prints `hello` (any line) and then sleeps forever
    /// — for spawn-rejection cases where the handshake fails before any
    /// invoke, so the child is killed by [`PluginSession`]'s drop.
    fn hello_plugin(dir: &Path, name: &str, hello: &str) -> PathBuf {
        let body = format!("#!/bin/sh\necho '{}'\nwhile :; do sleep 1; done\n", hello);
        write_script(dir, name, &body)
    }

    #[test]
    fn spawn_happy_path_captures_name_and_manifest() {
        let dir = tempdir().unwrap();
        let script = fake_plugin(dir.path(), "fake.sh", "", "", "", "");
        let session = spawn_script(&script).expect("spawn the fake plugin");
        assert_eq!(session.name, "fake");
        let manifest = session.manifest.clone().expect("hello carried a manifest");
        assert_eq!(manifest.name, "fake");
        assert_eq!(manifest.description, "Fake test plugin");
        session.shutdown().expect("graceful shutdown");
    }

    #[test]
    fn spawn_ack_reaches_the_plugin_as_its_first_stdin_line() {
        let dir = tempdir().unwrap();
        let ack_log = dir.path().join("ack.log");
        // Capture the first stdin line (the host's hello-ack) into a sidecar.
        let vars = format!("ACK_FILE='{}'\nACK_CAPTURED=''\n", ack_log.display());
        let first_read = "  if [ -z \"$ACK_CAPTURED\" ]; then\n    ACK_CAPTURED=1\n    echo \"$line\" >> \"$ACK_FILE\"\n  fi\n";
        let script = fake_plugin(dir.path(), "fake.sh", &vars, first_read, "", "");
        let session = spawn_script(&script).expect("spawn the fake plugin");
        session.shutdown().expect("graceful shutdown");
        // After the plugin has exited, its first stdin line — the ack it read —
        // is recorded; assert it is the preview hello exactly as spawn sends it.
        let received = std::fs::read_to_string(&ack_log)
            .expect("the plugin recorded the ack it read")
            .trim()
            .to_owned();
        let msg: Msg = serde_json::from_str(&received).expect("the ack is a protocol message");
        match msg {
            Msg::Hello {
                v,
                name,
                caps,
                manifest,
            } => {
                assert_eq!(v, API_VERSION, "ack announces the same protocol version");
                assert_eq!(name, "preview", "ack names the preview host");
                assert!(caps.is_empty(), "preview serves no host ops");
                assert!(manifest.is_none(), "preview carries no manifest");
            }
            other => panic!("expected the hello ack, got {other:?}"),
        }
    }

    #[test]
    fn spawn_rejects_a_bad_protocol_version() {
        let dir = tempdir().unwrap();
        let script = hello_plugin(
            dir.path(),
            "bad-version.sh",
            r#"{"t":"hello","v":2,"name":"fake","caps":["command:fake"]}"#,
        );
        let err = spawn_script(&script)
            .err()
            .expect("a bad version must fail the spawn");
        let text = err.to_string();
        assert!(
            text.contains("protocol version 2"),
            "the version mismatch is reported, got: {text}"
        );
    }

    #[test]
    fn spawn_rejects_an_unknown_host_capability() {
        let dir = tempdir().unwrap();
        let script = hello_plugin(
            dir.path(),
            "bad-caps.sh",
            r#"{"t":"hello","v":1,"name":"fake","caps":["host.frobnicate"]}"#,
        );
        let err = spawn_script(&script)
            .err()
            .expect("an unknown host cap must fail the spawn");
        let text = format!("{err:#}");
        assert!(
            text.contains("unknown host capability `host.frobnicate`"),
            "the unknown cap is reported, got: {text}"
        );
    }

    #[test]
    fn invoke_returns_the_matching_data_payload() {
        let dir = tempdir().unwrap();
        let script = fake_plugin(dir.path(), "fake.sh", "", "", "", "");
        let mut session = spawn_script(&script).expect("spawn the fake plugin");
        let data = session.invoke("fake", json!({})).expect("invoke answered");
        assert_eq!(data, json!({"echo": "pong"}));
        session.shutdown().expect("graceful shutdown");
    }

    #[test]
    fn interleaved_host_call_is_answered_host_unavailable_before_the_answer() {
        let dir = tempdir().unwrap();
        let call_log = dir.path().join("calls.log");
        let vars = format!("CALL_LOG='{}'\n", call_log.display());
        let before_answer = "\
      echo '{\"t\":\"call\",\"id\":999,\"op\":\"kv.get\",\"args\":{\"key\":\"x\"}}'\n\
      read -r host_resp\n\
      echo \"$host_resp\" >> \"$CALL_LOG\"\n";
        let script = fake_plugin(dir.path(), "fake.sh", &vars, "", before_answer, "");
        let mut session = spawn_script(&script).expect("spawn the fake plugin");
        let data = session.invoke("fake", json!({})).expect("invoke answered");
        assert_eq!(
            data,
            json!({"echo": "pong"}),
            "the real answer still arrives"
        );
        let logged = std::fs::read_to_string(&call_log).unwrap();
        assert!(
            logged.contains("\"kind\":\"HostUnavailable\""),
            "the host call was rejected as HostUnavailable, got: {logged}"
        );
        session.shutdown().expect("graceful shutdown");
    }

    #[test]
    fn noise_ping_event_and_redundant_hello_are_skipped_before_the_answer() {
        let dir = tempdir().unwrap();
        let before_answer = "\
      echo '{\"t\":\"ping\"}'\n\
      echo '{\"t\":\"event\",\"name\":\"view.timeout\"}'\n\
      echo '{\"t\":\"hello\",\"v\":1,\"name\":\"fake\",\"caps\":[\"command:fake\"]}'\n";
        let script = fake_plugin(dir.path(), "fake.sh", "", "", before_answer, "");
        let mut session = spawn_script(&script).expect("spawn the fake plugin");
        let data = session.invoke("fake", json!({})).expect("invoke answered");
        assert_eq!(data, json!({"echo": "pong"}));
        session.shutdown().expect("graceful shutdown");
    }

    #[test]
    fn shutdown_sends_bye_and_waits_for_the_exit() {
        let dir = tempdir().unwrap();
        let bye_log = dir.path().join("bye.log");
        let vars = format!("BYE_FILE='{}'\n", bye_log.display());
        let on_bye = "      echo BYE >> \"$BYE_FILE\"\n";
        let script = fake_plugin(dir.path(), "fake.sh", &vars, "", "", on_bye);
        let session = spawn_script(&script).expect("spawn the fake plugin");
        session.shutdown().expect("graceful shutdown");
        assert_eq!(
            std::fs::read_to_string(&bye_log).unwrap().trim(),
            "BYE",
            "the plugin observed the bye before exiting"
        );
    }

    #[test]
    fn drop_kills_a_rogue_child_written_before_hello() {
        let dir = tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let body = format!(
            "#!/bin/sh\necho $$ > '{}'\necho '{}'\nwhile :; do sleep 1; done\n",
            pid_file.display(),
            FAKE_HELLO
        );
        let script = write_script(dir.path(), "rogue.sh", &body);
        let session = spawn_script(&script).expect("spawn the rogue plugin");
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        drop(session);
        // The rogue never reads stdin, so nothing but the kill can end it;
        // the drop must reap it (no hang), and the process must be gone.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Path::new(&format!("/proc/{pid}")).exists() {
            assert!(
                Instant::now() < deadline,
                "the rogue child {pid} is still alive after the drop"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
