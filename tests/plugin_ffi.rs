// Shared test modules (common/) are compiled independently per integration test
// binary — each binary sees different subsets as "unused".
#![allow(dead_code)]

use std::ffi::CStr;
use std::ffi::CString;
use std::sync::Arc;

use pwr_bot::bot::host_ctx::PoiseHostCtx;
use pwr_bot::bot::plugin::ffi_host_ctx::FfiHostCtx;
use pwr_bot::bot::plugin::ffi_host_ctx::{self};
use pwr_bot::bot::plugin::loader::LoadedPlugin;
use pwr_bot::bot::plugin::registry::PluginRegistry;
use pwr_bot_sdk::InvokeRequest;
use pwr_bot_sdk::InvokeResponse;
use pwr_bot_sdk::PluginVTable;
use pwr_bot_sdk::ResponsePayload;

mod common;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_plugin() -> LoadedPlugin {
    unsafe { common::test_helpers::load_test_plugin() }
}

fn setup_system_ctx() -> Arc<PoiseHostCtx> {
    let data = common::test_helpers::test_data();
    common::test_helpers::setup_system_ctx(data)
}

fn reset_globals() {
    common::test_helpers::reset_host_registry();
}

/// Direct FFI invoke — bypasses `dispatch` (no HTTP), calls the vtable directly.
unsafe fn ffi_invoke(
    vtable: &PluginVTable,
    command: &str,
    args_json: &str,
    host_ctx: &Arc<PoiseHostCtx>,
) -> String {
    let cmd_c = CString::new(command).unwrap();
    let args_c = CString::new(args_json).unwrap();
    let ffi_ctx = FfiHostCtx::new(host_ctx.clone());

    let request = InvokeRequest {
        command: cmd_c.as_ptr(),
        args_json: args_c.as_ptr(),
        callbacks: ffi_host_ctx::host_callbacks() as *const _,
        ctx_handle: ffi_ctx.handle(),
    };
    let mut response = InvokeResponse {
        payload_json: std::ptr::null_mut(),
        error: std::ptr::null_mut(),
    };

    unsafe { (vtable.invoke)(&request as *const _, &mut response as *mut _) };

    if !response.error.is_null() {
        let err = unsafe { CStr::from_ptr(response.error) }
            .to_str()
            .unwrap()
            .to_string();
        if let Some(free) = vtable.free_string {
            unsafe { free(response.error) };
        }
        panic!("FFI invoke returned error: {err}");
    }

    let json_str = unsafe { CStr::from_ptr(response.payload_json) }
        .to_str()
        .unwrap()
        .to_string();
    if let Some(free) = vtable.free_string {
        unsafe { free(response.payload_json) };
    }
    json_str
}

/// Direct FFI invoke returning the raw `InvokeResponse` (for error-path tests).
unsafe fn ffi_invoke_raw(
    vtable: &PluginVTable,
    command: &str,
    args_json: &str,
    host_ctx: &Arc<PoiseHostCtx>,
) -> InvokeResponse {
    let cmd_c = CString::new(command).unwrap();
    let args_c = CString::new(args_json).unwrap();
    let ffi_ctx = FfiHostCtx::new(host_ctx.clone());

    let request = InvokeRequest {
        command: cmd_c.as_ptr(),
        args_json: args_c.as_ptr(),
        callbacks: ffi_host_ctx::host_callbacks() as *const _,
        ctx_handle: ffi_ctx.handle(),
    };
    let mut response = InvokeResponse {
        payload_json: std::ptr::null_mut(),
        error: std::ptr::null_mut(),
    };

    unsafe { (vtable.invoke)(&request as *const _, &mut response as *mut _) };
    response
}

// ---------------------------------------------------------------------------
// Loader tests
// ---------------------------------------------------------------------------

#[test]
fn parses_basic_metadata() {
    let plugin = load_plugin();

    assert_eq!(plugin.name, "test_plugin");
    assert_eq!(plugin.description, "Integration test plugin for pwr-bot");
    assert_eq!(plugin.version, "0.1.0");
}

#[test]
fn parses_command_declarations() {
    let plugin = load_plugin();

    assert_eq!(plugin.metadata.commands.len(), 4);
    assert_eq!(plugin.metadata.commands[0].name, "echo");
    assert_eq!(plugin.metadata.commands[1].name, "config_check");
    assert_eq!(plugin.metadata.commands[2].name, "return_error");
    assert_eq!(plugin.metadata.commands[3].name, "publish");
}

#[test]
fn parses_event_and_settings_declarations() {
    let plugin = load_plugin();

    assert_eq!(plugin.metadata.event_handlers.len(), 1);
    assert_eq!(plugin.metadata.event_handlers[0].event_name, "test.event");

    assert_eq!(plugin.metadata.settings_panels.len(), 1);
    assert_eq!(plugin.metadata.settings_panels[0].id, "test_plugin");
}

#[test]
fn parses_test_steps_and_tasks() {
    let plugin = load_plugin();

    assert_eq!(plugin.metadata.test_steps.len(), 1);
    assert_eq!(plugin.metadata.test_steps[0].command, "echo");

    assert_eq!(plugin.metadata.tasks.len(), 1);
    assert_eq!(plugin.metadata.tasks[0].name, "echo_task");
}

#[test]
fn load_plugin_rejects_missing_symbol() {
    let bad_path = std::env::temp_dir().join("nonexistent_plugin.so");
    let result = unsafe { pwr_bot::bot::plugin::loader::load_plugin(&bad_path) };
    match result {
        Err(err) => {
            assert!(err.contains("dlopen failed"), "unexpected error: {err}");
        }
        Ok(_) => panic!("expected load error for nonexistent .so"),
    }
}

// ---------------------------------------------------------------------------
// Registry tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn register_populates_registry() {
    reset_globals();
    let plugin = load_plugin();
    let registry = Arc::new(PluginRegistry::new());
    let cmds = registry.register(plugin).await;

    assert_eq!(cmds.len(), 4);
    assert_eq!(cmds[0].name, "echo");
    assert_eq!(cmds[1].name, "config_check");
    assert_eq!(cmds[2].name, "return_error");
    assert_eq!(cmds[3].name, "publish");

    let (_, p) = registry.lookup("echo").await.unwrap();
    assert_eq!(p.name, "test_plugin");

    assert!(registry.lookup("nonexistent").await.is_none());
}

#[tokio::test]
async fn registry_exposes_settings_panels() {
    reset_globals();
    let plugin = load_plugin();
    let registry = Arc::new(PluginRegistry::new());
    registry.register(plugin).await;

    let panels = registry.all_settings_panels().await;
    assert_eq!(panels.len(), 1);
    assert_eq!(panels[0].2.id, "test_plugin");
}

#[tokio::test]
async fn registry_exposes_event_handlers() {
    reset_globals();
    let plugin = load_plugin();
    let registry = Arc::new(PluginRegistry::new());
    registry.register(plugin).await;

    let handlers = registry.all_event_handlers().await;
    assert_eq!(handlers.len(), 1);
    assert_eq!(handlers[0].1.event_name, "test.event");
}

#[tokio::test]
async fn registry_exposes_test_steps() {
    reset_globals();
    let plugin = load_plugin();
    let registry = Arc::new(PluginRegistry::new());
    registry.register(plugin).await;

    let steps = registry.all_test_steps().await;
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].2.command, "echo");
}

#[tokio::test]
async fn registry_exposes_tasks() {
    reset_globals();
    let plugin = load_plugin();
    let registry = Arc::new(PluginRegistry::new());
    registry.register(plugin).await;

    let tasks = registry.all_tasks().await;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].1.name, "echo_task");
}

// ---------------------------------------------------------------------------
// FFI invocation tests (direct vtable calls, no HTTP dependency)
// ---------------------------------------------------------------------------

#[test]
fn ffi_invoke_echo_returns_payload() {
    reset_globals();
    let host_ctx = setup_system_ctx();
    let plugin = load_plugin();

    let json = unsafe { ffi_invoke(plugin.vtable, "echo", r#"{}"#, &host_ctx) };

    let payload: ResponsePayload = serde_json::from_str(&json).unwrap();
    assert_eq!(payload.content.unwrap(), "echoed: echo with args {}");
}

#[test]
fn ffi_invoke_config_check_returns_config_values() {
    reset_globals();
    let data = common::test_helpers::test_data();
    let host_ctx = common::test_helpers::setup_system_ctx(data);
    let plugin = load_plugin();

    let json = unsafe { ffi_invoke(plugin.vtable, "config_check", r#"{}"#, &host_ctx) };

    let payload: ResponsePayload = serde_json::from_str(&json).unwrap();
    let content: serde_json::Value = serde_json::from_str(&payload.content.unwrap()).unwrap();

    assert_eq!(content["poll_interval_secs"], 42);
    assert!(content["feature_enabled"].as_bool().unwrap());
    assert!(
        content["data_path"]
            .as_str()
            .unwrap()
            .contains("pwr-bot-test-data")
    );
}

#[test]
fn ffi_invoke_returns_error() {
    reset_globals();
    let host_ctx = setup_system_ctx();
    let plugin = load_plugin();

    let response = unsafe { ffi_invoke_raw(plugin.vtable, "return_error", r#"{}"#, &host_ctx) };

    assert!(!response.error.is_null(), "expected error but got success");
    assert!(response.payload_json.is_null());

    let err = unsafe { CStr::from_ptr(response.error) }
        .to_str()
        .unwrap()
        .to_string();
    if let Some(free) = plugin.vtable.free_string {
        unsafe { free(response.error) };
    }
    if !response.payload_json.is_null()
        && let Some(free) = plugin.vtable.free_string
    {
        unsafe { free(response.payload_json) };
    }

    assert_eq!(err, "intentional error for testing");
}

#[test]
fn ffi_init_succeeds() {
    reset_globals();
    let host_ctx = setup_system_ctx();
    let plugin = load_plugin();

    let empty = CString::new("").unwrap();
    let ffi_ctx = FfiHostCtx::new(host_ctx);
    let mut response = InvokeResponse {
        payload_json: std::ptr::null_mut(),
        error: std::ptr::null_mut(),
    };

    unsafe {
        (plugin.vtable.init.as_ref().unwrap())(
            &InvokeRequest {
                command: empty.as_ptr(),
                args_json: empty.as_ptr(),
                callbacks: ffi_host_ctx::host_callbacks() as *const _,
                ctx_handle: ffi_ctx.handle(),
            } as *const _,
            &mut response as *mut _,
        );
    }

    assert!(response.error.is_null(), "init returned error: unexpected");
}

#[test]
fn ffi_on_event_succeeds() {
    reset_globals();
    let host_ctx = setup_system_ctx();
    let plugin = load_plugin();

    let event_name = CString::new("test.event").unwrap();
    let payload = CString::new(r#"{"value":42}"#).unwrap();
    let ffi_ctx = FfiHostCtx::new(host_ctx);

    let success = unsafe {
        (plugin.vtable.on_event.as_ref().unwrap())(
            event_name.as_ptr(),
            payload.as_ptr(),
            ffi_host_ctx::host_callbacks() as *const _,
            ffi_ctx.handle(),
        )
    };

    assert!(success, "on_event returned false");
}

#[test]
fn ffi_shutdown_succeeds() {
    let plugin = load_plugin();

    let success = unsafe { (plugin.vtable.shutdown.as_ref().unwrap())() };

    assert!(success, "shutdown returned false");
}

// ---------------------------------------------------------------------------
// Event bus integration test
// ---------------------------------------------------------------------------

#[test]
fn publish_event_callback_reaches_event_bus() {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    reset_globals();
    let received = Arc::new(AtomicBool::new(false));
    let received_clone = received.clone();

    let data = common::test_helpers::test_data();
    data.event_bus.subscribe_named(
        "plugin.test",
        Box::new(move |payload| {
            assert_eq!(payload["from"], "plugin");
            received_clone.store(true, Ordering::SeqCst);
            Ok(())
        }),
    );

    let host_ctx = common::test_helpers::setup_system_ctx(data);
    let plugin = load_plugin();

    let _ = unsafe {
        ffi_invoke(
            plugin.vtable,
            "publish",
            r#"{"event":"plugin.test"}"#,
            &host_ctx,
        )
    };

    assert!(
        received.load(Ordering::SeqCst),
        "event callback was not invoked"
    );
}
