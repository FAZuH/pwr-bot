// Host-side harness: proves the full spawn -> hello -> invoke -> interact ->
// error -> bye round trip against the typed fixture (see
// `src/bin/hello_plugin.rs`). Sent messages are typed [`Msg`] values, not
// hand-written JSON strings: the fixture must be driven by the typed envelope.
use std::time::Duration;

use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::BUTTON_CUSTOM_ID;
use pwr_plugin_protocol::Msg;
use pwr_plugin_protocol::PLUGIN_NAME;
use serde_json::json;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::ChildStdin;
use tokio::process::ChildStdout;
use tokio::process::Command;

async fn next_line(reader: &mut BufReader<ChildStdout>) -> String {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .expect("read line from plugin stdout");
    line
}

/// Writes one protocol line to the plugin's stdin. Deliberately non-flushing:
/// tokio `ChildStdin` is unbuffered, so there is nothing to flush (the
/// fixture's `write_msg` must flush because piped stdout is block-buffered).
async fn send_line(stdin: &mut ChildStdin, msg: &Msg) {
    let mut line = serde_json::to_string(msg).expect("serialize msg");
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .await
        .expect("write to plugin stdin");
}

#[tokio::test]
async fn hello_plugin_round_trip() {
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        let mut child = Command::new(env!("CARGO_BIN_EXE_hello_plugin"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn hello_plugin");
        let mut stdin: ChildStdin = child.stdin.take().expect("plugin stdin");
        let stdout: ChildStdout = child.stdout.take().expect("plugin stdout");
        let mut reader = BufReader::new(stdout);

        // 1. Handshake: the plugin announces itself first.
        let hello: Msg = serde_json::from_str(next_line(&mut reader).await.trim()).expect("hello");
        let Msg::Hello { v, name, caps } = hello else {
            panic!("expected hello, got {hello:?}");
        };
        assert_eq!(v, API_VERSION);
        assert_eq!(name, PLUGIN_NAME);
        assert!(caps.iter().any(|c| c == "command:hello"));

        // 2. Invoke -> view payload.
        send_line(
            &mut stdin,
            &Msg::Call {
                id: 1,
                op: "invoke".into(),
                cmd: Some(PLUGIN_NAME.into()),
                args: Some(json!({})),
            },
        )
        .await;
        let resp: Msg =
            serde_json::from_str(next_line(&mut reader).await.trim()).expect("invoke resp");
        let Msg::Resp {
            id,
            ok,
            data,
            error,
        } = resp
        else {
            panic!("expected resp, got {resp:?}");
        };
        assert_eq!(id, 1);
        assert!(ok, "invoke must succeed");
        assert!(error.is_none(), "invoke must carry no error");
        let data = data.expect("invoke data");
        assert_eq!(data["content"], "Hello from plugin!");
        assert_eq!(
            data["components"][0]["components"][0]["custom_id"],
            BUTTON_CUSTOM_ID
        );
        assert_eq!(data["flags"], 0);

        // 3. Component round-trip: re-render with bumped counter.
        send_line(
            &mut stdin,
            &Msg::Call {
                id: 2,
                op: "view.interact".into(),
                cmd: Some(PLUGIN_NAME.into()),
                args: Some(json!({
                    "custom_id": BUTTON_CUSTOM_ID,
                    "values": [],
                    "user_id": 123,
                    "channel_id": 456,
                    "guild_id": 789
                })),
            },
        )
        .await;
        let resp: Msg =
            serde_json::from_str(next_line(&mut reader).await.trim()).expect("interact resp");
        let Msg::Resp { id, ok, data, .. } = resp else {
            panic!("expected resp, got {resp:?}");
        };
        assert_eq!(id, 2);
        assert!(ok, "interact must succeed");
        assert!(
            data.expect("interact data")["content"]
                .as_str()
                .expect("content string")
                .contains("clicked"),
            "interact content must mention the click"
        );

        // 4. Unknown action -> first-class error on the wire.
        send_line(
            &mut stdin,
            &Msg::Call {
                id: 3,
                op: "view.interact".into(),
                cmd: Some(PLUGIN_NAME.into()),
                args: Some(json!({"custom_id": "nope"})),
            },
        )
        .await;
        let resp: Msg =
            serde_json::from_str(next_line(&mut reader).await.trim()).expect("error resp");
        let Msg::Resp {
            id,
            ok,
            data,
            error,
        } = resp
        else {
            panic!("expected resp, got {resp:?}");
        };
        assert_eq!(id, 3);
        assert!(!ok, "unknown action must fail");
        assert!(data.is_none(), "failed resp must carry no data");
        let error = error.expect("error resp must carry an error");
        assert_eq!(error.kind, "UnknownAction");
        assert_eq!(error.msg, "unknown custom_id");

        // 5. One-way timeout event: never answered, so no line may arrive.
        send_line(
            &mut stdin,
            &Msg::Event {
                name: "view.timeout".into(),
                data: Some(json!({})),
            },
        )
        .await;
        // Timing-based negative assertion; known flake risk on loaded CI.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut line = String::new();
        let no_reply =
            tokio::time::timeout(Duration::from_millis(100), reader.read_line(&mut line))
                .await
                .is_err();
        assert!(no_reply, "view.timeout must never be answered");

        // 6. Graceful unload: bye, then closing stdin lets the plugin exit 0.
        send_line(&mut stdin, &Msg::Bye).await;
        drop(stdin);
        let status = child.wait().await.expect("wait for plugin exit");
        assert!(status.success(), "plugin exited with {status}");
    })
    .await;

    result.expect("hello_plugin round trip finished within 15s");
}
