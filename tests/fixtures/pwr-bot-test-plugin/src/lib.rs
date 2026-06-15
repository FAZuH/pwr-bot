use std::ffi::CStr;
use std::ffi::CString;

use pwr_bot_sdk::InvokeRequest;
use pwr_bot_sdk::InvokeResponse;
use pwr_bot_sdk::PluginHost;
use pwr_bot_sdk::PluginMetadata;
use pwr_bot_sdk::PluginVTable;
use pwr_bot_sdk::PWR_BOT_PLUGIN_API_VERSION;

use pwr_bot_sdk::ResponsePayload;

// Plugin instance stored as a global singleton.
static PLUGIN: TestPlugin = TestPlugin;

struct TestPlugin;

impl TestPlugin {
    fn metadata_json(&self) -> *mut std::ffi::c_char {
        let meta = PluginMetadata {
            api_version: PWR_BOT_PLUGIN_API_VERSION,
            name: "test_plugin".into(),
            description: "Integration test plugin for pwr-bot".into(),
            version: "0.1.0".into(),
            commands: vec![
                pwr_bot_sdk::CommandSpec {
                    name: "echo".into(),
                    description: "Echoes the command and args".into(),
                    args: vec![],
                },
                pwr_bot_sdk::CommandSpec {
                    name: "config_check".into(),
                    description: "Returns host config values".into(),
                    args: vec![],
                },
                pwr_bot_sdk::CommandSpec {
                    name: "return_error".into(),
                    description: "Returns an error".into(),
                    args: vec![],
                },
                pwr_bot_sdk::CommandSpec {
                    name: "publish".into(),
                    description: "Publishes a test event".into(),
                    args: vec![pwr_bot_sdk::ArgSpec {
                        name: "event".into(),
                        description: "Event name to publish".into(),
                        kind: "String".into(),
                    }],
                },
            ],
            event_handlers: vec![pwr_bot_sdk::EventHandlerSpec::new("test.event".into())],
            settings_panels: vec![pwr_bot_sdk::SettingsPanelSpec::new(
                "test_plugin",
                "Test Plugin",
            )],
            test_steps: vec![pwr_bot_sdk::TestStepSpec::new(
                "echo_test",
                "Test the echo command",
                "echo",
                serde_json::json!({}),
            )],
            tasks: vec![pwr_bot_sdk::TaskSpec::new("echo_task", 3600, "echo")],
        };
        let json = serde_json::to_string(&meta).unwrap_or_else(|_| "{}".to_string());
        CString::new(json).unwrap().into_raw()
    }

    fn handle_invoke(
        &self,
        command: &str,
        args: serde_json::Value,
        host: &PluginHost,
    ) -> Result<ResponsePayload, String> {
        match command {
            "echo" => {
                let content = format!("echoed: {command} with args {args}");
                Ok(ResponsePayload {
                    content: Some(content),
                    ephemeral: false,
                    components_json: None,
                    embed_json: None,
                })
            }
            "config_check" => {
                let poll_interval = host.poll_interval().as_secs();
                let data_path = host
                    .data_path()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let feature_enabled = host.is_feature_enabled("test_feature");
                let content = serde_json::json!({
                    "poll_interval_secs": poll_interval,
                    "data_path": data_path,
                    "feature_enabled": feature_enabled,
                })
                .to_string();
                Ok(ResponsePayload {
                    content: Some(content),
                    ephemeral: false,
                    components_json: None,
                    embed_json: None,
                })
            }
            "return_error" => Err("intentional error for testing".into()),
            "publish" => {
                let event_name = args
                    .get("event")
                    .and_then(|v| v.as_str())
                    .unwrap_or("plugin.test");
                unsafe {
                    host.publish_event(event_name, r#"{"from":"plugin"}"#)
                        .map_err(|e| format!("publish_event failed: {e}"))?;
                }
                Ok(ResponsePayload {
                    content: None,
                    ephemeral: false,
                    components_json: None,
                    embed_json: None,
                })
            }
            _ => Err(format!("unknown command: {command}")),
        }
    }
}

// ---------------------------------------------------------------------------
// FFI entry point — manually constructed (no export_plugin! macro)
// ---------------------------------------------------------------------------

unsafe extern "C" fn plugin_metadata() -> *mut std::ffi::c_char {
    PLUGIN.metadata_json()
}

unsafe extern "C" fn plugin_free_string(s: *mut std::ffi::c_char) {
    if !s.is_null() {
        let _ = unsafe { CString::from_raw(s) };
    }
}

unsafe extern "C" fn plugin_invoke(
    req: *const InvokeRequest,
    resp: *mut InvokeResponse,
) {
    let req = unsafe { &*req };
    let command = unsafe { CStr::from_ptr(req.command) }
        .to_str()
        .unwrap_or("");
    let args_json = unsafe { CStr::from_ptr(req.args_json) }
        .to_str()
        .unwrap_or("{}");
    let args: serde_json::Value =
        serde_json::from_str(args_json).unwrap_or(serde_json::Value::Null);
    let host = unsafe { PluginHost::new(req.ctx_handle, &*req.callbacks) };

    match PLUGIN.handle_invoke(command, args, &host) {
        Ok(payload) => {
            let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
            let c_str = CString::new(json).unwrap();
            unsafe {
                (*resp).payload_json = c_str.into_raw();
                (*resp).error = std::ptr::null_mut();
            }
        }
        Err(err) => {
            let c_str = CString::new(err).unwrap();
            unsafe {
                (*resp).payload_json = std::ptr::null_mut();
                (*resp).error = c_str.into_raw();
            }
        }
    }
}

unsafe extern "C" fn plugin_init(
    _req: *const InvokeRequest,
    resp: *mut InvokeResponse,
) {
    unsafe {
        (*resp).payload_json = std::ptr::null_mut();
        (*resp).error = std::ptr::null_mut();
    }
}

unsafe extern "C" fn plugin_shutdown() -> bool {
    true
}

unsafe extern "C" fn plugin_on_event(
    _event_name: *const std::ffi::c_char,
    _payload_json: *const std::ffi::c_char,
    _callbacks: *const pwr_bot_sdk::HostCallbacks,
    _ctx_handle: u64,
) -> bool {
    true
}

/// Entry point called by the host to retrieve the plugin's [`PluginVTable`].
///
/// # Safety
///
/// The returned pointer must not be dereferenced after the library is unloaded.
/// The host guarantees this by leaking the library handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pwr_bot_plugin_entry() -> *const PluginVTable {
    static VTABLE: PluginVTable = PluginVTable {
        api_version: PWR_BOT_PLUGIN_API_VERSION,
        metadata: plugin_metadata,
        invoke: plugin_invoke,
        init: Some(plugin_init),
        shutdown: Some(plugin_shutdown),
        on_event: Some(plugin_on_event),
        free_string: Some(plugin_free_string),
    };
    std::ptr::addr_of!(VTABLE)
}
