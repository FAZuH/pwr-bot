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

/// Registers plugin event handlers on the event bus.
///
/// For each plugin's declared [`EventHandlerSpec`], subscribes to the named
/// event and dispatches it to the plugin's [`BotPlugin::on_event`] method.
pub fn register_plugin_event_handlers(
    event_bus: &crate::event::event_bus::EventBus,
    plugins: &[Arc<dyn BotPlugin + 'static>],
) {
    for plugin in plugins {
        let plugin_name = plugin.name().to_string();
        for spec in plugin.event_handlers() {
            let event_name = spec.event_name.clone();
            let en = event_name.clone();
            let p = plugin.clone();
            event_bus.subscribe_named(
                &event_name,
                Box::new(move |payload| {
                    let plugin = p.clone();
                    let event_name = en.clone();
                    tokio::spawn(async move {
                        let _ = dispatch_on_event(&*plugin, &event_name, payload).await;
                    });
                    Ok(())
                }),
            );
            log::info!(
                "Registered event handler '{}' for plugin '{plugin_name}'",
                event_name,
            );
        }
    }
}

/// Spawns background tasks declared by builtin plugins.
///
/// Each [`TaskSpec`] returned by a plugin's [`BotPlugin::tasks`] is spawned
/// as a tokio interval that calls [`BotPlugin::invoke`] at the specified rate.
/// The system context must be initialized before calling this.
pub async fn dispatch_tasks(plugins: &[Arc<dyn BotPlugin + 'static>]) {
    let system_ctx = match host_registry::system_ctx() {
        Some(ctx) => ctx.clone(),
        None => {
            log::error!("Cannot dispatch tasks: system context not initialized");
            return;
        }
    };

    for plugin in plugins {
        let plugin_name = plugin.name().to_string();
        for task in plugin.tasks() {
            let plugin = plugin.clone();
            let ctx = system_ctx.clone();
            let command = task.command.clone();
            let interval_secs = task.interval_secs;

            tokio::spawn(async move {
                let mut timer = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
                loop {
                    timer.tick().await;
                    let handle = host_registry::register(ctx.clone());
                    let host = PluginHost::new(handle, ffi_host_ctx::host_callbacks());
                    let _ = plugin.invoke(&command, serde_json::Value::Null, &host).await;
                    host_registry::unregister(handle);
                }
            });

            log::info!(
                "Spawned task '{}' for plugin '{plugin_name}' (interval: {interval_secs}s)",
                task.name,
            );
        }
    }
}
