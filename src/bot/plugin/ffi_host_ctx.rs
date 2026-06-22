//! FFI host context implementation.
//!
//! Implements [`HostCtx`] for the plugin FFI path by wrapping the host
//! callbacks and providing them to plugins through the `pwr_bot_sdk`
//! callback table.

use std::ffi::CStr;
use std::ffi::CString;
use std::sync::Arc;

use async_trait::async_trait;
use poise::serenity_prelude::*;

use crate::bot::Data;
use crate::bot::command::Error;
use crate::bot::host_ctx::HostCtx;
use crate::bot::host_ctx::MessagePayload;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::host_registry;

/// Global callback table shared by all FFI invocations.
static HOST_CALLBACKS: std::sync::OnceLock<pwr_bot_sdk::HostCallbacks> = std::sync::OnceLock::new();

/// Returns the global callback table.
pub fn host_callbacks() -> &'static pwr_bot_sdk::HostCallbacks {
    HOST_CALLBACKS.get_or_init(|| pwr_bot_sdk::HostCallbacks {
        send_reply: cb_send_reply,
        edit_reply: cb_edit_reply,
        defer: cb_defer,
        get_guild_id: cb_guild_id,
        get_author_id: cb_author_id,
        get_channel_id: cb_channel_id,
        free_string: cb_free_string,
        send_channel_message: cb_send_channel_message,
        send_dm: cb_send_dm,
        publish_event: cb_publish_event,
        get_poll_interval: cb_get_poll_interval,
        get_data_path: cb_get_data_path,
        is_feature_enabled: cb_is_feature_enabled,
    })
}

/// FFI-backed implementation of [`HostCtx`].
///
/// Each invocation gets a unique handle that plugins can use to call back
/// into the host. The host stores the actual [`PoiseHostCtx`] and dispatches
/// callback requests to it.
pub struct FfiHostCtx {
    handle: u64,
    inner: Arc<PoiseHostCtx>,
}

impl FfiHostCtx {
    pub fn new(inner: Arc<PoiseHostCtx>) -> Self {
        let handle = host_registry::register(inner.clone());
        Self { handle, inner }
    }

    pub fn handle(&self) -> u64 {
        self.handle
    }
}

impl Drop for FfiHostCtx {
    fn drop(&mut self) {
        host_registry::unregister(self.handle);
    }
}

// ---------------------------------------------------------------------------
// FFI callbacks
// ---------------------------------------------------------------------------

/// Shared runtime for host callbacks called from within a plugin's
/// `Runtime::block_on`. We cannot nest `Handle::block_on` on the host's
/// runtime (the thread is already inside it), so we run each future on a
/// dedicated thread with its own tokio runtime.
static HOST_BLOCK_RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

fn host_block_on<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = HOST_BLOCK_RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .expect("Failed to create host block_on runtime")
        });
        let _ = tx.send(rt.block_on(f));
    });
    rx.recv().expect("host runtime task failed")
}

fn ctx_data(ctx_handle: u64) -> Option<Arc<Data>> {
    host_registry::get(ctx_handle).map(|ctx| ctx.data())
}

unsafe extern "C" fn cb_send_reply(
    ctx_handle: u64,
    reply_json: *const std::ffi::c_char,
    out_message_id: *mut u64,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let ctx = match host_registry::get(ctx_handle) {
        Some(ctx) => ctx,
        None => return false,
    };

    let json_str = match unsafe { CStr::from_ptr(reply_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let payload: pwr_bot_sdk::ResponsePayload = match serde_json::from_str(json_str) {
        Ok(p) => p,
        Err(e) => {
            let err = CString::new(format!("invalid reply JSON: {e}")).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            return false;
        }
    };

    match host_block_on(async move { ctx.send_message(&MessagePayload(payload.data)).await }) {
        Ok(msg_id) => {
            if !out_message_id.is_null() {
                unsafe { *out_message_id = msg_id.get() };
            }
            true
        }
        Err(e) => {
            let err = CString::new(e.to_string()).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            false
        }
    }
}

unsafe extern "C" fn cb_edit_reply(
    ctx_handle: u64,
    message_id: u64,
    reply_json: *const std::ffi::c_char,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let ctx = match host_registry::get(ctx_handle) {
        Some(ctx) => ctx,
        None => return false,
    };

    let json_str = match unsafe { CStr::from_ptr(reply_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let payload: pwr_bot_sdk::ResponsePayload = match serde_json::from_str(json_str) {
        Ok(p) => p,
        Err(e) => {
            let err = CString::new(format!("invalid reply JSON: {e}")).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            return false;
        }
    };

    let mid = message_id;
    match host_block_on(async move {
        ctx.edit_message(MessageId::new(mid), &MessagePayload(payload.data))
            .await
    }) {
        Ok(_) => true,
        Err(e) => {
            let err = CString::new(e.to_string()).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            false
        }
    }
}

unsafe extern "C" fn cb_defer(ctx_handle: u64) -> bool {
    let ctx = match host_registry::get(ctx_handle) {
        Some(ctx) => ctx,
        None => return false,
    };

    // Fast path: if already responded (e.g. by dispatch() on the main runtime),
    // return immediately without spawning a thread / runtime for a no-op.
    if ctx.was_responded() {
        return true;
    }

    host_block_on(async move { ctx.defer().await }).is_ok()
}

unsafe extern "C" fn cb_guild_id(ctx_handle: u64) -> u64 {
    host_registry::get(ctx_handle)
        .and_then(|ctx| ctx.guild_id())
        .unwrap_or(0)
}

unsafe extern "C" fn cb_author_id(ctx_handle: u64) -> u64 {
    host_registry::get(ctx_handle)
        .map(|ctx| ctx.author_id())
        .unwrap_or(0)
}

unsafe extern "C" fn cb_channel_id(ctx_handle: u64) -> u64 {
    host_registry::get(ctx_handle)
        .map(|ctx| ctx.channel_id())
        .unwrap_or(0)
}

unsafe extern "C" fn cb_free_string(s: *mut std::ffi::c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

// ---- Channel message callback ----

unsafe extern "C" fn cb_send_channel_message(
    ctx_handle: u64,
    channel_id: u64,
    payload_json: *const std::ffi::c_char,
    out_message_id: *mut u64,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let ctx = match host_registry::get(ctx_handle) {
        Some(ctx) => ctx,
        None => return false,
    };

    let json_str = match unsafe { CStr::from_ptr(payload_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let payload: pwr_bot_sdk::ResponsePayload = match serde_json::from_str(json_str) {
        Ok(p) => p,
        Err(e) => {
            let err = CString::new(format!("invalid payload JSON: {e}")).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            return false;
        }
    };

    let http = ctx.http().clone();
    let channel_id_val = channel_id;
    match host_block_on(async move {
        http.send_message(
            poise::serenity_prelude::ChannelId::new(channel_id_val).into(),
            vec![],
            &payload.data,
        )
        .await
    }) {
        Ok(msg) => {
            unsafe { *out_message_id = msg.id.get() };
            true
        }
        Err(e) => {
            let err = CString::new(e.to_string()).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            false
        }
    }
}

// ---- DM callback ----

unsafe extern "C" fn cb_send_dm(
    ctx_handle: u64,
    user_id: u64,
    payload_json: *const std::ffi::c_char,
    out_message_id: *mut u64,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let ctx = match host_registry::get(ctx_handle) {
        Some(ctx) => ctx,
        None => return false,
    };

    let json_str = match unsafe { CStr::from_ptr(payload_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let payload: pwr_bot_sdk::ResponsePayload = match serde_json::from_str(json_str) {
        Ok(p) => p,
        Err(e) => {
            let err = CString::new(format!("invalid DM payload JSON: {e}")).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            return false;
        }
    };

    let http = ctx.http().clone();
    let user_id_val = user_id;
    match host_block_on(async move {
        let dm_channel = UserId::new(user_id_val).create_dm_channel(&http).await?;
        http.send_message(dm_channel.id.into(), vec![], &payload.data)
            .await
    }) {
        Ok(msg) => {
            unsafe { *out_message_id = msg.id.get() };
            true
        }
        Err(e) => {
            let err = CString::new(e.to_string()).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            false
        }
    }
}

// ---- Event callback ----

unsafe extern "C" fn cb_publish_event(
    ctx_handle: u64,
    event_name: *const std::ffi::c_char,
    payload_json: *const std::ffi::c_char,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let name = match unsafe { CStr::from_ptr(event_name) }.to_str() {
        Ok(n) => n,
        Err(_) => return false,
    };
    let payload_str = match unsafe { CStr::from_ptr(payload_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let payload: serde_json::Value = match serde_json::from_str(payload_str) {
        Ok(v) => v,
        Err(e) => {
            let err = CString::new(format!("invalid event payload: {e}")).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            return false;
        }
    };

    let data = match ctx_data(ctx_handle) {
        Some(d) => d,
        None => return false,
    };

    match data.event_bus.publish_named(name, payload) {
        Ok(()) => true,
        Err(e) => {
            let err = CString::new(e.to_string()).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            false
        }
    }
}

// ---- Config callbacks ----

unsafe extern "C" fn cb_get_poll_interval(ctx_handle: u64) -> u64 {
    ctx_data(ctx_handle)
        .map(|data| data.config.poll_interval.as_secs())
        .unwrap_or(60)
}

unsafe extern "C" fn cb_get_data_path(ctx_handle: u64, out: *mut *mut std::ffi::c_char) -> bool {
    match ctx_data(ctx_handle) {
        Some(data) => {
            let path = data.config.data_path.to_string_lossy().to_string();
            let c_str = CString::new(path).unwrap();
            unsafe { *out = c_str.into_raw() };
            true
        }
        None => false,
    }
}

unsafe extern "C" fn cb_is_feature_enabled(
    ctx_handle: u64,
    feature: *const std::ffi::c_char,
) -> bool {
    let feature_name = match unsafe { CStr::from_ptr(feature) }.to_str() {
        Ok(n) => n,
        Err(_) => return false,
    };

    let data = match ctx_data(ctx_handle) {
        Some(d) => d,
        None => return false,
    };

    data.config.features.is_enabled(feature_name)
}

// ---------------------------------------------------------------------------
// HostCtx impl (used by built-in commands, not FFI)
// ---------------------------------------------------------------------------

#[async_trait]
impl HostCtx for FfiHostCtx {
    fn guild_id(&self) -> Option<u64> {
        self.inner.guild_id()
    }

    fn author_id(&self) -> u64 {
        self.inner.author_id()
    }

    fn channel_id(&self) -> u64 {
        self.inner.channel_id()
    }

    fn data(&self) -> Arc<Data> {
        self.inner.data()
    }

    async fn defer(&self) -> Result<(), Error> {
        self.inner.defer().await
    }

    async fn send_message(&self, payload: &MessagePayload) -> Result<MessageId, Error> {
        self.inner.send_message(payload).await
    }

    async fn edit_message(
        &self,
        message_id: MessageId,
        payload: &MessagePayload,
    ) -> Result<(), Error> {
        self.inner.edit_message(message_id, payload).await
    }

    async fn acknowledge(&self, interaction: &ComponentInteraction) -> Result<(), Error> {
        self.inner.acknowledge(interaction).await
    }
}
