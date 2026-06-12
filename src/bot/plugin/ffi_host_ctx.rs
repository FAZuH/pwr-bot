//! FFI host context implementation.
//!
//! Implements [`HostCtx`] for the plugin FFI path by wrapping the host
//! callbacks and providing them to plugins through the `pwr_bot_sdk`
//! callback table.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use async_trait::async_trait;
use poise::serenity_prelude::*;

use crate::bot::Data;
use crate::bot::command::Error;
use crate::bot::host_ctx::HostCtx;
use crate::bot::host_ctx::MessagePayload;
use crate::bot::host_ctx::PoiseHostCtx;

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// FFI-backed implementation of [`HostCtx`].
///
/// Each invocation gets a unique handle that plugins can use to call back
/// into the host. The host stores the actual [`PoiseHostCtx`] and dispatches
/// callback requests to it.
pub struct FfiHostCtx {
    handle: u64,
    inner: Arc<PoiseHostCtx>,
    /// Cached callback table for this instance.
    ///
    /// Stored as a static reference so the pointer remains valid for the
    /// duration of the FFI call.
    pub callbacks: pwr_bot_sdk::HostCallbacks,
}

impl FfiHostCtx {
    pub fn new(inner: Arc<PoiseHostCtx>) -> Self {
        let handle = NEXT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        Self {
            handle,
            inner,
            callbacks: Self::create_callbacks(),
        }
    }

    pub fn handle(&self) -> u64 {
        self.handle
    }

    fn create_callbacks() -> pwr_bot_sdk::HostCallbacks {
        pwr_bot_sdk::HostCallbacks {
            send_reply: Self::cb_send_reply,
            edit_reply: Self::cb_edit_reply,
            defer: Self::cb_defer,
            get_guild_id: Self::cb_guild_id,
            get_author_id: Self::cb_author_id,
            get_channel_id: Self::cb_channel_id,
            query_db: Self::cb_query_db,
            free_string: Self::cb_free_string,
        }
    }

    extern "C" fn cb_send_reply(
        _ctx_handle: u64,
        _reply_json: *const std::ffi::c_char,
        _out_err: *mut *mut std::ffi::c_char,
    ) -> bool {
        // TODO: implement
        false
    }

    extern "C" fn cb_edit_reply(
        _ctx_handle: u64,
        _message_id: u64,
        _reply_json: *const std::ffi::c_char,
        _out_err: *mut *mut std::ffi::c_char,
    ) -> bool {
        false
    }

    extern "C" fn cb_defer(_ctx_handle: u64) -> bool {
        false
    }

    extern "C" fn cb_guild_id(_ctx_handle: u64) -> u64 {
        0
    }

    extern "C" fn cb_author_id(_ctx_handle: u64) -> u64 {
        0
    }

    extern "C" fn cb_channel_id(_ctx_handle: u64) -> u64 {
        0
    }

    extern "C" fn cb_query_db(
        _ctx_handle: u64,
        _sql: *const std::ffi::c_char,
        _params_json: *const std::ffi::c_char,
        _out_json: *mut *mut std::ffi::c_char,
        _out_err: *mut *mut std::ffi::c_char,
    ) -> bool {
        false
    }

    extern "C" fn cb_free_string(s: *mut std::ffi::c_char) {
        if !s.is_null() {
            unsafe {
                let _ = std::ffi::CString::from_raw(s);
            }
        }
    }
}

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
