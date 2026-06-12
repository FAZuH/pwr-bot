//! Handles FFI invocation dispatch for plugin commands.

use std::ffi::CString;
use std::sync::Arc;

use pwr_bot_sdk::InvokeRequest;
use pwr_bot_sdk::InvokeResponse;
use pwr_bot_sdk::ResponsePayload;

use crate::bot::command::Error;
use crate::bot::host_ctx::HostCtx;
use crate::bot::host_ctx::MessagePayload;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::ffi_host_ctx::FfiHostCtx;

/// Dispatches a plugin command invocation.
///
/// Called from the poise command handler. Serializes arguments, calls the
/// plugin's `invoke()` function via FFI, and processes the response.
pub async fn dispatch(
    host_ctx: &Arc<PoiseHostCtx>,
    plugin_vtable: &pwr_bot_sdk::PluginVTable,
    command: &str,
    args_json: &str,
) -> Result<(), Error> {
    // Create the FFI host context for this invocation
    let ffi_ctx = Arc::new(FfiHostCtx::new(host_ctx.clone()));

    let cmd_c = CString::new(command).map_err(|e| e.to_string())?;
    let args_c = CString::new(args_json).map_err(|e| e.to_string())?;

    let request = InvokeRequest {
        command: cmd_c.as_ptr() as *const std::ffi::c_char,
        args_json: args_c.as_ptr() as *const std::ffi::c_char,
        callbacks: &ffi_ctx.callbacks as *const _,
        ctx_handle: ffi_ctx.handle(),
    };

    let mut response = InvokeResponse {
        payload_json: std::ptr::null_mut(),
        error: std::ptr::null_mut(),
    };

    unsafe {
        (plugin_vtable.invoke)(&request as *const _, &mut response as *mut _);

        // Process the response
        if !response.error.is_null() {
            let err = std::ffi::CStr::from_ptr(response.error)
                .to_str()
                .unwrap_or("unknown error")
                .to_string();
            (plugin_vtable.free_response)(&mut response);
            return Err(err.into());
        }

        if !response.payload_json.is_null() {
            let json_str = std::ffi::CStr::from_ptr(response.payload_json)
                .to_str()
                .unwrap_or("{}");

            if let Ok(payload) = serde_json::from_str::<ResponsePayload>(json_str) {
                // Send the response to Discord
                let msg_payload = MessagePayload {
                    content: payload.content,
                    embed: None, // Embed from JSON would need deserialization
                    ephemeral: payload.ephemeral,
                };
                host_ctx.send_message(&msg_payload).await?;
            }

            (plugin_vtable.free_response)(&mut response);
        }
    }

    Ok(())
}
