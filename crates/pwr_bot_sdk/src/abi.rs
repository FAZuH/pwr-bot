use std::ffi::CStr;
use std::ffi::c_char;
use std::fmt;

pub const PWR_BOT_PLUGIN_API_VERSION: u32 = 3;

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
    // -- Interaction callbacks --
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
    pub free_string: unsafe extern "C" fn(s: *mut c_char),

    // -- Channel message callback --
    pub send_channel_message: unsafe extern "C" fn(
        ctx_handle: u64,
        channel_id: u64,
        payload_json: *const c_char,
        out_message_id: *mut u64,
        out_err: *mut *mut c_char,
    ) -> bool,

    // -- Event callback --
    pub publish_event: unsafe extern "C" fn(
        ctx_handle: u64,
        event_name: *const c_char,
        payload_json: *const c_char,
        out_err: *mut *mut c_char,
    ) -> bool,

    // -- DM callback --
    pub send_dm: unsafe extern "C" fn(
        ctx_handle: u64,
        user_id: u64,
        payload_json: *const c_char,
        out_message_id: *mut u64,
        out_err: *mut *mut c_char,
    ) -> bool,

    // -- Config callbacks --
    pub get_poll_interval: unsafe extern "C" fn(ctx_handle: u64) -> u64,
    pub get_data_path: unsafe extern "C" fn(ctx_handle: u64, out: *mut *mut c_char) -> bool,
    pub is_feature_enabled: unsafe extern "C" fn(ctx_handle: u64, feature: *const c_char) -> bool,
}

/// Stable C ABI vtable that every plugin exports.
#[repr(C)]
pub struct PluginVTable {
    pub api_version: u32,
    pub metadata: unsafe extern "C" fn() -> *mut c_char,
    pub invoke: unsafe extern "C" fn(req: *const InvokeRequest, resp: *mut InvokeResponse),
    pub init: Option<unsafe extern "C" fn(req: *const InvokeRequest, resp: *mut InvokeResponse)>,
    pub shutdown: Option<unsafe extern "C" fn() -> bool>,
    pub on_event: Option<
        unsafe extern "C" fn(
            event_name: *const c_char,
            payload_json: *const c_char,
            callbacks: *const HostCallbacks,
            ctx_handle: u64,
        ) -> bool,
    >,
    pub free_string: Option<unsafe extern "C" fn(s: *mut c_char)>,
}

impl InvokeRequest {
    pub unsafe fn command_str(&self) -> &str {
        unsafe { CStr::from_ptr(self.command).to_str().unwrap_or("") }
    }

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
            .field(
                "send_channel_message",
                &(self.send_channel_message as *const ()),
            )
            .field("send_dm", &(self.send_dm as *const ()))
            .field("publish_event", &(self.publish_event as *const ()))
            .field("get_poll_interval", &(self.get_poll_interval as *const ()))
            .field("get_data_path", &(self.get_data_path as *const ()))
            .field(
                "is_feature_enabled",
                &(self.is_feature_enabled as *const ()),
            )
            .field("free_string", &(self.free_string as *const ()))
            .finish()
    }
}

impl fmt::Debug for PluginVTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginVTable")
            .field("api_version", &self.api_version)
            .field("metadata", &(self.metadata as *const ()))
            .field("invoke", &(self.invoke as *const ()))
            .field("init", &self.init.map(|f| f as *const ()))
            .field("shutdown", &self.shutdown.map(|f| f as *const ()))
            .field("on_event", &self.on_event.map(|f| f as *const ()))
            .field("free_string", &self.free_string.map(|f| f as *const ()))
            .finish()
    }
}
