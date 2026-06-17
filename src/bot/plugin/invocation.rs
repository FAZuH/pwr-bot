//! Handles FFI plugin invocation dispatch.

use std::ffi::CString;
use std::sync::Arc;

use poise::serenity_prelude::UserId;
use pwr_bot_sdk::InvokeRequest;
use pwr_bot_sdk::InvokeResponse;
use pwr_bot_sdk::ResponsePayload;
use tracing::instrument;

use crate::bot::command::Error;
use crate::bot::host_ctx::HostCtx;
use crate::bot::host_ctx::MessagePayload;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::ffi_host_ctx;
use crate::bot::plugin::ffi_host_ctx::FfiHostCtx;
use crate::bot::plugin::host_registry;
use crate::bot::plugin::loader::LoadedPlugin;
use crate::bot::plugin::registry::PluginRegistry;

/// Dispatches a plugin command invocation via FFI.
///
/// Defers the interaction on the main runtime first (reliable serenity HTTP path),
/// then marks the context as responded so the plugin's FFI `host.defer()` becomes
/// a no-op instead of going through `host_block_on` (which can deadlock reqwest).
///
/// The FFI interaction is scoped in a block so that [`InvokeRequest`] and
/// [`InvokeResponse`] (which contain raw pointers and are not `Send`) are
/// dropped before any `.await` point.
#[instrument(skip_all, fields(ffi.command = %command))]
pub async fn dispatch(
    host_ctx: &Arc<PoiseHostCtx>,
    plugin_vtable: &pwr_bot_sdk::PluginVTable,
    command: &str,
    args_json: &str,
) -> Result<(), Error> {
    tracing::debug!(ffi.command = %command, "dispatch: before defer");
    host_ctx.defer().await?;
    tracing::debug!(ffi.command = %command, "dispatch: after defer");

    let ffi_ctx = Arc::new(FfiHostCtx::new(host_ctx.clone()));

    let (msg_payload, has_components) = {
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
                tracing::debug!(ffi.command = %command, ffi.error = %err, "FFI command returned error");
                if let Some(free) = plugin_vtable.free_string {
                    free(response.error);
                }
                if !response.payload_json.is_null()
                    && let Some(free) = plugin_vtable.free_string
                {
                    free(response.payload_json);
                }
                return Err(err.into());
            }

            if !response.payload_json.is_null() {
                let json_str = std::ffi::CStr::from_ptr(response.payload_json)
                    .to_str()
                    .unwrap_or("{}")
                    .to_string();
                if let Some(free) = plugin_vtable.free_string {
                    free(response.payload_json);
                }

                let payload: Option<ResponsePayload> = serde_json::from_str(&json_str)
                    .map_err(|e| {
                        tracing::error!(
                            ffi.command = %command,
                            response_json = %json_str,
                            error = %e,
                            "failed to deserialize plugin response",
                        );
                        e
                    })
                    .ok();
                let has_components = payload
                    .as_ref()
                    .and_then(|p| p.data.get("components"))
                    .and_then(|c| c.as_array())
                    .is_some_and(|a| !a.is_empty());
                (payload.map(|p| MessagePayload(p.data)), has_components)
            } else {
                (None, false)
            }
        }
    };

    if let Some(msg) = msg_payload {
        tracing::debug!(ffi.command = %command, "dispatch: sending response");
        let msg_id = host_ctx.send_message(&msg).await?;
        tracing::debug!(
            ffi.command = %command,
            message.id = %msg_id,
            "dispatch: response sent",
        );

        // Register as interactive view if the response contained components
        if has_components {
            let data = host_ctx.data();
            let plugin_name = command.split_whitespace().next().unwrap_or(command);
            data.view_registry
                .register(plugin_name, msg_id, UserId::new(host_ctx.author_id()))
                .await;
            tracing::debug!(
                ffi.command = %command,
                plugin.name = %plugin_name,
                message.id = %msg_id,
                "plugin view registered",
            );
        }
    } else {
        tracing::debug!(ffi.command = %command, "dispatch: no response payload");
    }

    Ok(())
}

/// Dispatches a command to the correct plugin by looking it up from the registry.
pub async fn dispatch_plugin_command(
    registry: &PluginRegistry,
    host_ctx: &Arc<PoiseHostCtx>,
    command: &str,
    args: serde_json::Value,
) -> Result<(), Error> {
    let parent = command.split_whitespace().next().unwrap_or(command);
    let (_, plugin) = registry
        .lookup(parent)
        .await
        .ok_or_else(|| format!("Plugin not found for command: {parent}"))?;

    let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "null".to_string());
    dispatch(host_ctx, plugin.vtable, command, &args_json).await
}

/// Calls `init` on an FFI plugin using the system context.
///
/// Uses an empty C string for `command` and `args_json` since the
/// init path does not read these fields.
pub async fn dispatch_init_ffi(loaded: &LoadedPlugin) -> Result<(), String> {
    if loaded.vtable.init.is_none() {
        return Ok(());
    }

    let ctx =
        host_registry::system_ctx().ok_or_else(|| "system context not initialized".to_string())?;
    let ffi_ctx = Arc::new(FfiHostCtx::new(ctx.clone()));
    let empty = c"";
    let request = InvokeRequest {
        command: empty.as_ptr(),
        args_json: empty.as_ptr(),
        callbacks: ffi_host_ctx::host_callbacks() as *const _,
        ctx_handle: ffi_ctx.handle(),
    };
    let mut response = InvokeResponse {
        payload_json: std::ptr::null_mut(),
        error: std::ptr::null_mut(),
    };

    unsafe {
        (loaded.vtable.init.as_ref().unwrap())(&request as *const _, &mut response as *mut _);
    }

    if !response.error.is_null() {
        let err = unsafe { std::ffi::CStr::from_ptr(response.error) }
            .to_str()
            .unwrap_or("unknown error")
            .to_string();
        if let Some(free) = loaded.vtable.free_string {
            unsafe { free(response.error) };
        }
        return Err(err);
    }

    Ok(())
}

/// Registers all FFI plugin event handlers on the event bus.
///
/// For each plugin's declared [`EventHandlerSpec`](pwr_bot_sdk::EventHandlerSpec)s,
/// subscribes a named handler that dispatches to the plugin's `on_event` via FFI.
pub fn register_ffi_event_handlers(
    event_bus: &crate::event::event_bus::EventBus,
    ffi_plugins: &[Arc<LoadedPlugin>],
) {
    for plugin in ffi_plugins {
        for spec in &plugin.metadata.event_handlers {
            let event_name = spec.event_name.clone();
            let en = event_name.clone();
            let p = plugin.clone();
            event_bus.subscribe_named(
                &event_name,
                Box::new(move |payload| {
                    let plugin = p.clone();
                    let event_name = en.clone();
                    tokio::spawn(async move {
                        let _ = dispatch_on_event_ffi(&plugin, &event_name, payload).await;
                    });
                    Ok(())
                }),
            );
            tracing::info!(
                event.name = %event_name,
                plugin.name = %plugin.name,
                "event handler registered",
            );
        }
    }
}

/// Dispatches an event to an FFI plugin's `on_event` handler.
pub async fn dispatch_on_event_ffi(
    loaded: &LoadedPlugin,
    event_name: &str,
    payload: serde_json::Value,
) -> Result<(), String> {
    if loaded.vtable.on_event.is_none() {
        return Ok(());
    }

    let ctx =
        host_registry::system_ctx().ok_or_else(|| "system context not initialized".to_string())?;
    let ffi_ctx = Arc::new(FfiHostCtx::new(ctx.clone()));

    let event_name_c = CString::new(event_name).map_err(|e| e.to_string())?;
    let payload_c = serde_json::to_string(&payload)
        .map_err(|e| e.to_string())
        .and_then(|s| CString::new(s).map_err(|e| e.to_string()))?;

    unsafe {
        let success = (loaded.vtable.on_event.as_ref().unwrap())(
            event_name_c.as_ptr(),
            payload_c.as_ptr(),
            ffi_host_ctx::host_callbacks() as *const _,
            ffi_ctx.handle(),
        );
        if success {
            Ok(())
        } else {
            Err("FFI on_event returned false".to_string())
        }
    }
}

/// Spawns background tasks declared by FFI plugins.
pub async fn dispatch_tasks_ffi(ffi_plugins: &[Arc<LoadedPlugin>]) {
    let system_ctx = match host_registry::system_ctx() {
        Some(ctx) => ctx.clone(),
        None => {
            tracing::error!("cannot dispatch FFI tasks: system context not initialized");
            return;
        }
    };

    for plugin in ffi_plugins {
        let vtable: &'static pwr_bot_sdk::PluginVTable = plugin.vtable;
        for task in &plugin.metadata.tasks {
            let ctx = system_ctx.clone();
            let command = task.command.clone();
            let interval_secs = task.interval_secs;
            let task_name = task.name.clone();
            let plugin_name = plugin.name.clone();
            let name_for_log = plugin_name.clone();

            tokio::spawn(async move {
                let mut timer =
                    tokio::time::interval(std::time::Duration::from_secs(interval_secs));
                loop {
                    timer.tick().await;
                    let ffi_ctx = Arc::new(FfiHostCtx::new(ctx.clone()));
                    let args_c = c"null";
                    let cmd_c = match CString::new(command.as_str()) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    // Scope FFI interaction to drop raw-pointer types before await
                    unsafe {
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
                        (vtable.invoke)(&request as *const _, &mut response as *mut _);
                        if !response.error.is_null() {
                            let err = std::ffi::CStr::from_ptr(response.error)
                                .to_str()
                                .unwrap_or("unknown error");
                            tracing::error!(
                                task.name = %task_name,
                                plugin.name = %plugin_name,
                                ffi.error = %err,
                                "FFI task failed",
                            );
                            if let Some(free) = vtable.free_string {
                                free(response.error);
                            }
                        }
                        if !response.payload_json.is_null()
                            && let Some(free) = vtable.free_string
                        {
                            free(response.payload_json);
                        }
                    }
                }
            });

            tracing::info!(
                task.name = %name_for_log,
                task.interval = interval_secs,
                "FFI task spawned",
            );
        }
    }
}
