use pwr_bot::plugin::RunningPlugin;
use pwr_plugin_protocol::Msg;
use serde_json::json;

mod probe;
use probe::probe_binary;

#[tokio::test]
async fn running_plugin_forwards_progress_before_the_final_response() {
    let plugin = RunningPlugin::spawn(probe_binary("arg-echo-plugin"))
        .await
        .expect("spawn progress fixture");
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();

    let response = plugin
        .call_with_progress(
            "invoke",
            Some("arg-echo"),
            Some(json!({ "emit_progress": true })),
            progress_tx,
        )
        .await
        .expect("progress call completes");
    let progress = progress_rx.recv().await.expect("progress payload");

    assert_eq!(progress["view"], json!({ "phase": "working" }));
    assert!(matches!(response, Msg::Resp { ok: true, .. }));
    plugin.stop().await.expect("stop progress fixture");
}
