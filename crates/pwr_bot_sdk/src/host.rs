use std::ffi::CString;
use std::path::PathBuf;
use std::time::Duration;

use crate::abi::HostCallbacks;
use crate::plugin::DbValue;

/// Safe wrapper around the FFI host callbacks.
pub struct PluginHost {
    ctx_handle: u64,
    callbacks: &'static HostCallbacks,
}

impl PluginHost {
    pub fn new(ctx_handle: u64, callbacks: &'static HostCallbacks) -> Self {
        Self {
            ctx_handle,
            callbacks,
        }
    }

    pub fn ctx_handle(&self) -> u64 {
        self.ctx_handle
    }

    // ---- Interaction callbacks ----

    /// Sends a reply to the interaction.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call. The `reply_json` must be valid
    /// UTF-8 (it will be null-terminated by `CString`).
    pub unsafe fn send_reply(&self, reply_json: &str) -> Result<(), String> {
        let c_str = CString::new(reply_json).map_err(|e| e.to_string())?;
        let mut out_err: *mut std::ffi::c_char = std::ptr::null_mut();
        let ok =
            unsafe { (self.callbacks.send_reply)(self.ctx_handle, c_str.as_ptr(), &mut out_err) };
        if ok {
            Ok(())
        } else if !out_err.is_null() {
            let err = unsafe { CString::from_raw(out_err) }
                .into_string()
                .unwrap_or_else(|_| "unknown error".to_string());
            Err(err)
        } else {
            Err("unknown error".to_string())
        }
    }

    /// Edits a previously sent reply.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call. `reply_json` must be valid UTF-8.
    pub unsafe fn edit_reply(&self, message_id: u64, reply_json: &str) -> Result<(), String> {
        let c_str = CString::new(reply_json).map_err(|e| e.to_string())?;
        let mut out_err: *mut std::ffi::c_char = std::ptr::null_mut();
        let ok = unsafe {
            (self.callbacks.edit_reply)(self.ctx_handle, message_id, c_str.as_ptr(), &mut out_err)
        };
        if ok {
            Ok(())
        } else if !out_err.is_null() {
            let err = unsafe { CString::from_raw(out_err) }
                .into_string()
                .unwrap_or_else(|_| "unknown error".to_string());
            Err(err)
        } else {
            Err("unknown error".to_string())
        }
    }

    /// Defers the interaction, showing a loading state.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call.
    pub unsafe fn defer(&self) -> Result<(), String> {
        if unsafe { (self.callbacks.defer)(self.ctx_handle) } {
            Ok(())
        } else {
            Err("defer failed".to_string())
        }
    }

    /// Returns the guild ID for this invocation.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call.
    pub unsafe fn guild_id(&self) -> u64 {
        unsafe { (self.callbacks.get_guild_id)(self.ctx_handle) }
    }

    /// Returns the author's user ID for this invocation.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call.
    pub unsafe fn author_id(&self) -> u64 {
        unsafe { (self.callbacks.get_author_id)(self.ctx_handle) }
    }

    /// Returns the channel ID for this invocation.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call.
    pub unsafe fn channel_id(&self) -> u64 {
        unsafe { (self.callbacks.get_channel_id)(self.ctx_handle) }
    }

    // ---- Database callbacks ----

    /// Executes a parameterized SQL query and returns the result rows as JSON.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call. `sql` must be valid
    /// UTF-8 (it will be null-terminated).
    pub unsafe fn query_db(
        &self,
        sql: &str,
        params: &[DbValue],
    ) -> Result<serde_json::Value, String> {
        let sql_c = CString::new(sql).map_err(|e| e.to_string())?;
        let params_json =
            serde_json::to_string(params).map_err(|e| format!("param serialization: {e}"))?;
        let params_c = CString::new(params_json).map_err(|e| e.to_string())?;
        let mut out_json: *mut std::ffi::c_char = std::ptr::null_mut();
        let mut out_err: *mut std::ffi::c_char = std::ptr::null_mut();
        let ok = unsafe {
            (self.callbacks.query_db)(
                self.ctx_handle,
                sql_c.as_ptr(),
                params_c.as_ptr(),
                &mut out_json,
                &mut out_err,
            )
        };
        if ok && !out_json.is_null() {
            let result = unsafe { CString::from_raw(out_json) }
                .into_string()
                .unwrap_or_else(|_| "".to_string());
            serde_json::from_str(&result).map_err(|e| format!("JSON parse: {e}"))
        } else if !out_err.is_null() {
            let err = unsafe { CString::from_raw(out_err) }
                .into_string()
                .unwrap_or_else(|_| "unknown error".to_string());
            Err(err)
        } else {
            Err("unknown error".to_string())
        }
    }

    /// Executes a parameterized SQL DML statement and returns the number of rows affected.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call. `sql` must be valid
    /// UTF-8 (it will be null-terminated).
    pub unsafe fn execute_db(&self, sql: &str, params: &[DbValue]) -> Result<u64, String> {
        let sql_c = CString::new(sql).map_err(|e| e.to_string())?;
        let params_json =
            serde_json::to_string(params).map_err(|e| format!("param serialization: {e}"))?;
        let params_c = CString::new(params_json).map_err(|e| e.to_string())?;
        let mut out_rows: u64 = 0;
        let mut out_err: *mut std::ffi::c_char = std::ptr::null_mut();
        let ok = unsafe {
            (self.callbacks.execute_db)(
                self.ctx_handle,
                sql_c.as_ptr(),
                params_c.as_ptr(),
                &mut out_rows,
                &mut out_err,
            )
        };
        if ok {
            Ok(out_rows)
        } else if !out_err.is_null() {
            let err = unsafe { CString::from_raw(out_err) }
                .into_string()
                .unwrap_or_else(|_| "unknown error".to_string());
            Err(err)
        } else {
            Err("unknown error".to_string())
        }
    }

    // ---- Channel message callback ----

    /// Sends a message to a specific channel (not an interaction reply).
    ///
    /// Returns the message ID on success.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call.
    pub unsafe fn send_channel_message(
        &self,
        channel_id: u64,
        payload_json: &str,
    ) -> Result<u64, String> {
        let c_str = CString::new(payload_json).map_err(|e| e.to_string())?;
        let mut out_message_id: u64 = 0;
        let mut out_err: *mut std::ffi::c_char = std::ptr::null_mut();
        let ok = unsafe {
            (self.callbacks.send_channel_message)(
                self.ctx_handle,
                channel_id,
                c_str.as_ptr(),
                &mut out_message_id,
                &mut out_err,
            )
        };
        if ok {
            Ok(out_message_id)
        } else if !out_err.is_null() {
            let err = unsafe { CString::from_raw(out_err) }
                .into_string()
                .unwrap_or_else(|_| "unknown error".to_string());
            Err(err)
        } else {
            Err("unknown error".to_string())
        }
    }

    // ---- DM callback ----

    /// Sends a direct message to a user.
    ///
    /// Returns the message ID on success.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call.
    pub unsafe fn send_dm(
        &self,
        user_id: u64,
        payload_json: &str,
    ) -> Result<u64, String> {
        let c_str = CString::new(payload_json).map_err(|e| e.to_string())?;
        let mut out_message_id: u64 = 0;
        let mut out_err: *mut std::ffi::c_char = std::ptr::null_mut();
        let ok = unsafe {
            (self.callbacks.send_dm)(
                self.ctx_handle,
                user_id,
                c_str.as_ptr(),
                &mut out_message_id,
                &mut out_err,
            )
        };
        if ok {
            Ok(out_message_id)
        } else if !out_err.is_null() {
            let err = unsafe { CString::from_raw(out_err) }
                .into_string()
                .unwrap_or_else(|_| "unknown error".to_string());
            Err(err)
        } else {
            Err("unknown error".to_string())
        }
    }

    // ---- Event callback ----

    /// Publishes an event to the event bus by name.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call.
    pub unsafe fn publish_event(&self, event_name: &str, payload_json: &str) -> Result<(), String> {
        let name_c = CString::new(event_name).map_err(|e| e.to_string())?;
        let payload_c = CString::new(payload_json).map_err(|e| e.to_string())?;
        let mut out_err: *mut std::ffi::c_char = std::ptr::null_mut();
        let ok = unsafe {
            (self.callbacks.publish_event)(
                self.ctx_handle,
                name_c.as_ptr(),
                payload_c.as_ptr(),
                &mut out_err,
            )
        };
        if ok {
            Ok(())
        } else if !out_err.is_null() {
            let err = unsafe { CString::from_raw(out_err) }
                .into_string()
                .unwrap_or_else(|_| "unknown error".to_string());
            Err(err)
        } else {
            Err("unknown error".to_string())
        }
    }

    // ---- Config accessors ----

    /// Returns the configured poll interval for background tasks.
    pub fn poll_interval(&self) -> Duration {
        let secs = unsafe { (self.callbacks.get_poll_interval)(self.ctx_handle) };
        Duration::from_secs(secs)
    }

    /// Returns the data path for file storage.
    pub fn data_path(&self) -> Result<PathBuf, String> {
        let mut out: *mut std::ffi::c_char = std::ptr::null_mut();
        if unsafe { (self.callbacks.get_data_path)(self.ctx_handle, &mut out) } {
            let path = unsafe { CString::from_raw(out) }
                .into_string()
                .unwrap_or_else(|_| ".".to_string());
            Ok(PathBuf::from(path))
        } else {
            Err("failed to get data path".to_string())
        }
    }

    /// Checks whether a named feature is enabled.
    pub fn is_feature_enabled(&self, feature: &str) -> bool {
        let c_str = CString::new(feature).unwrap_or_default();
        unsafe { (self.callbacks.is_feature_enabled)(self.ctx_handle, c_str.as_ptr()) }
    }
}

unsafe impl Send for PluginHost {}
unsafe impl Sync for PluginHost {}
