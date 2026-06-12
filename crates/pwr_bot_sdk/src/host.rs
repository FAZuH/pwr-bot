use std::ffi::CString;

use crate::abi::HostCallbacks;

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

    /// Executes a database query via the host's connection pool.
    ///
    /// # Safety
    ///
    /// The registered [`HostCallbacks`] must outlive the call. `sql` and `params_json` must be
    /// valid UTF-8 (they will be null-terminated).
    pub unsafe fn query_db(&self, sql: &str, params_json: &str) -> Result<String, String> {
        let sql_c = CString::new(sql).map_err(|e| e.to_string())?;
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
            Ok(result)
        } else if !out_err.is_null() {
            let err = unsafe { CString::from_raw(out_err) }
                .into_string()
                .unwrap_or_else(|_| "unknown error".to_string());
            Err(err)
        } else {
            Err("unknown error".to_string())
        }
    }
}

unsafe impl Send for PluginHost {}
unsafe impl Sync for PluginHost {}
