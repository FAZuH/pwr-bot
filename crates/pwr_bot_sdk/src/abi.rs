use std::ffi::CStr;
use std::ffi::c_char;
use std::fmt;

pub const PWR_BOT_PLUGIN_API_VERSION: u32 = 1;

pub const PWR_BOT_PLUGIN_ENTRY: &[u8] = b"pwr_bot_plugin_entry\0";

#[repr(C)]
pub struct InvokeRequest {
    pub command: *const c_char,
    pub args_json: *const c_char,
    pub callbacks: *const HostCallbacks,
    pub ctx_handle: u64,
}

#[repr(C)]
pub struct InvokeResponse {
    pub payload_json: *mut c_char,
    pub error: *mut c_char,
}

/// JSON-encoded array of command descriptors.
#[repr(C)]
pub struct CommandList {
    pub json: *const c_char,
}

/// Callback table the host provides to plugins.
#[repr(C)]
pub struct HostCallbacks {
    pub send_reply: unsafe extern "C" fn(
        ctx_handle: u64,
        reply_json: *const c_char,
        out_err: *mut *mut c_char,
    ) -> bool,
    pub edit_reply: unsafe extern "C" fn(
        ctx_handle: u64,
        message_id: u64,
        reply_json: *const c_char,
        out_err: *mut *mut c_char,
    ) -> bool,
    pub defer: unsafe extern "C" fn(ctx_handle: u64) -> bool,
    pub get_guild_id: unsafe extern "C" fn(ctx_handle: u64) -> u64,
    pub get_author_id: unsafe extern "C" fn(ctx_handle: u64) -> u64,
    pub get_channel_id: unsafe extern "C" fn(ctx_handle: u64) -> u64,
    pub query_db: unsafe extern "C" fn(
        ctx_handle: u64,
        sql: *const c_char,
        params_json: *const c_char,
        out_json: *mut *mut c_char,
        out_err: *mut *mut c_char,
    ) -> bool,
    pub free_string: unsafe extern "C" fn(s: *mut c_char),
}

/// Stable C ABI vtable that every plugin exports.
#[repr(C)]
pub struct PluginVTable {
    pub api_version: u32,
    pub commands: unsafe extern "C" fn() -> CommandList,
    pub invoke: unsafe extern "C" fn(req: *const InvokeRequest, resp: *mut InvokeResponse),
    pub free_command_list: unsafe extern "C" fn(list: CommandList),
    pub free_response: unsafe extern "C" fn(resp: *mut InvokeResponse),
}

impl InvokeRequest {
    /// Returns the command name string.
    ///
    /// # Safety
    ///
    /// `self.command` must be a valid, null-terminated C string pointer
    /// that remains valid for the duration of the call.
    pub unsafe fn command_str(&self) -> &str {
        unsafe { CStr::from_ptr(self.command).to_str().unwrap_or("") }
    }

    /// Returns the JSON arguments string.
    ///
    /// # Safety
    ///
    /// `self.args_json` must be a valid, null-terminated C string pointer
    /// that remains valid for the duration of the call.
    pub unsafe fn args_json_str(&self) -> &str {
        unsafe { CStr::from_ptr(self.args_json).to_str().unwrap_or("") }
    }
}

unsafe impl Send for HostCallbacks {}
unsafe impl Sync for HostCallbacks {}
unsafe impl Send for PluginVTable {}
unsafe impl Sync for PluginVTable {}

impl fmt::Debug for HostCallbacks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostCallbacks")
            .field("send_reply", &(self.send_reply as *const ()))
            .field("edit_reply", &(self.edit_reply as *const ()))
            .field("defer", &(self.defer as *const ()))
            .field("get_guild_id", &(self.get_guild_id as *const ()))
            .field("get_author_id", &(self.get_author_id as *const ()))
            .field("get_channel_id", &(self.get_channel_id as *const ()))
            .field("query_db", &(self.query_db as *const ()))
            .field("free_string", &(self.free_string as *const ()))
            .finish()
    }
}

impl fmt::Debug for PluginVTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginVTable")
            .field("api_version", &self.api_version)
            .field("commands", &(self.commands as *const ()))
            .field("invoke", &(self.invoke as *const ()))
            .field("free_command_list", &(self.free_command_list as *const ()))
            .field("free_response", &(self.free_response as *const ()))
            .finish()
    }
}
