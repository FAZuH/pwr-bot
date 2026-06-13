//! Handles FFI and builtin plugin invocation dispatch.

use std::ffi::CString;
use std::sync::Arc;

use pwr_bot_sdk::BotPlugin;
use pwr_bot_sdk::InvokeRequest;
use pwr_bot_sdk::InvokeResponse;
use pwr_bot_sdk::PluginHost;
use pwr_bot_sdk::ResponsePayload;

use crate::bot::command::Error;
use crate::bot::host_ctx::HostCtx;
use crate::bot::host_ctx::MessagePayload;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::ffi_host_ctx;
use crate::bot::plugin::ffi_host_ctx::FfiHostCtx;
use crate::bot::plugin::host_registry;

/// Dispatches a plugin command invocation via FFI (for `.so` plugins).
pub async fn dispatch(
    host_ctx: &Arc<PoiseHostCtx>,
    plugin_vtable: &pwr_bot_sdk::PluginVTable,
    command: &str,
    args_json: &str,
) -> Result<(), Error> {
    let ffi_ctx = Arc::new(FfiHostCtx::new(host_ctx.clone()));

    let cmd_c = CString::new(command).map_err(|e| e.to_string())?;
    let args_c = CString::new(args_json).map_err(|e| e.to_string())?;

    let request = InvokeRequest {
        command: cmd_c.as_ptr() as *const std::ffi::c_char,
        args_json: args_c.as_ptr() as *const std::ffi::c_char,
        callbacks: ffi_host_ctx::host_callbacks() as *const _,
        ctx_handle: ffi_ctx.handle(),
    };

    let mut response = InvokeResponse {
        payload_json: std::ptr::null_mut(),
        error: std::ptr::null_mut(),
    };

    unsafe {
        (plugin_vtable.invoke)(&request as *const _, &mut response as *mut _);

        if !response.error.is_null() {
            let err = std::ffi::CStr::from_ptr(response.error)
                .to_str()
                .unwrap_or("unknown error")
                .to_string();
            if let Some(free) = plugin_vtable.free_string {
                free(response.error);
            }
            if !response.payload_json.is_null() {
                if let Some(free) = plugin_vtable.free_string {
                    free(response.payload_json);
                }
            }
            return Err(err.into());
        }

        if !response.payload_json.is_null() {
            let json_str = std::ffi::CStr::from_ptr(response.payload_json)
                .to_str()
                .unwrap_or("{}");

            if let Ok(payload) = serde_json::from_str::<ResponsePayload>(json_str) {
                let msg_payload = MessagePayload {
                    content: payload.content,
                    embed: None,
                    ephemeral: payload.ephemeral,
                };
                host_ctx.send_message(&msg_payload).await?;
            }

            if let Some(free) = plugin_vtable.free_string {
                free(response.payload_json);
            }
        }
    }

    Ok(())
}

/// Dispatches a compiled-in plugin command invocation.
///
/// Calls the plugin's `invoke()` directly (no FFI serialization) while still
/// providing the FFI-based `PluginHost` for callbacks.
pub async fn dispatch_builtin(
    host_ctx: &Arc<PoiseHostCtx>,
    plugin: &dyn BotPlugin,
    command: &str,
    args: serde_json::Value,
) -> Result<(), Error> {
    let handle = host_registry::register(host_ctx.clone());

    let host = PluginHost::new(handle, ffi_host_ctx::host_callbacks());

    let result = plugin.invoke(command, args, &host).await;
    host_registry::unregister(handle);

    let payload = result.map_err(|e| format!("Plugin error: {e}"))?;

    let msg_payload = MessagePayload {
        content: payload.content,
        embed: None,
        ephemeral: payload.ephemeral,
    };
    host_ctx.send_message(&msg_payload).await?;

    Ok(())
}

/// Calls `init` on a builtin plugin using the system context.
pub async fn dispatch_init(plugin: &dyn BotPlugin) -> Result<(), String> {
    let ctx = host_registry::system_ctx()
        .ok_or_else(|| "system context not initialized".to_string())?;
    let handle = host_registry::register(ctx.clone());
    let host = PluginHost::new(handle, ffi_host_ctx::host_callbacks());
    let result = plugin.init(&host).await;
    host_registry::unregister(handle);
    result
}

/// Calls `shutdown` on a builtin plugin.
pub async fn dispatch_shutdown(plugin: &dyn BotPlugin) -> Result<(), String> {
    plugin.shutdown().await
}

/// Dispatches an event to a builtin plugin's `on_event` handler.
pub async fn dispatch_on_event(
    plugin: &dyn BotPlugin,
    event_name: &str,
    payload: serde_json::Value,
) -> Result<(), String> {
    let ctx = host_registry::system_ctx()
        .ok_or_else(|| "system context not initialized".to_string())?;
    let handle = host_registry::register(ctx.clone());
    let host = PluginHost::new(handle, ffi_host_ctx::host_callbacks());
    let result = plugin.on_event(event_name, payload, &host).await;
    host_registry::unregister(handle);
    result
}
