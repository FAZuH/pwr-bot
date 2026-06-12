/// Generate the FFI glue for a plugin.
///
/// The plugin type must implement [`BotPlugin`] and be constructible as a
/// `const` expression (e.g. a unit struct or a struct where all fields are
/// `const`-compatible).
#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty, $initializer:expr) => {
        static PLUGIN: $plugin_type = $initializer;

        static COMMANDS_JSON: std::sync::OnceLock<*const std::ffi::c_char> =
            std::sync::OnceLock::new();

        fn build_commands_json() -> *const std::ffi::c_char {
            let specs = PLUGIN.commands();
            let json = serde_json::to_string(&specs).unwrap_or_else(|_| "[]".to_string());
            std::ffi::CString::new(json).unwrap().into_raw() as *const std::ffi::c_char
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwr_bot_plugin_entry() -> *const $crate::abi::PluginVTable {
            static VTABLE: $crate::abi::PluginVTable = $crate::abi::PluginVTable {
                api_version: $crate::abi::PWR_BOT_PLUGIN_API_VERSION,
                commands: plugin_commands,
                invoke: plugin_invoke,
                free_command_list: plugin_free_command_list,
                free_response: plugin_free_response,
            };
            std::ptr::addr_of!(VTABLE)
        }

        unsafe extern "C" fn plugin_commands() -> $crate::abi::CommandList {
            let ptr = COMMANDS_JSON
                .get()
                .copied()
                .unwrap_or_else(|| unsafe { *COMMANDS_JSON.get_or_init(build_commands_json) });
            $crate::abi::CommandList { json: ptr }
        }

        unsafe extern "C" fn plugin_free_command_list(list: $crate::abi::CommandList) {
            if !list.json.is_null() {
                if COMMANDS_JSON.get().map(|p| *p != list.json).unwrap_or(true) {
                    unsafe {
                        let _ = std::ffi::CString::from_raw(list.json as *mut std::ffi::c_char);
                    }
                }
            }
        }

        unsafe extern "C" fn plugin_invoke(
            req: *const $crate::abi::InvokeRequest,
            resp: *mut $crate::abi::InvokeResponse,
        ) {
            let req = unsafe { &*req };
            let command = unsafe {
                std::ffi::CStr::from_ptr(req.command)
                    .to_str()
                    .unwrap_or("")
                    .to_string()
            };
            let args_json = unsafe {
                std::ffi::CStr::from_ptr(req.args_json)
                    .to_str()
                    .unwrap_or("{}")
                    .to_string()
            };
            let host = $crate::host::PluginHost::new(req.ctx_handle, unsafe { &*req.callbacks });

            let args: serde_json::Value =
                serde_json::from_str(&args_json).unwrap_or(serde_json::Value::Null);

            let result = tokio::runtime::Handle::current()
                .block_on(async { PLUGIN.invoke(&command, args, &host).await });

            match result {
                Ok(payload) => {
                    let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
                    let c_str = std::ffi::CString::new(json).unwrap();
                    unsafe {
                        (*resp).payload_json = c_str.into_raw();
                        (*resp).error = std::ptr::null_mut();
                    }
                }
                Err(err) => {
                    let c_str = std::ffi::CString::new(err).unwrap();
                    unsafe {
                        (*resp).payload_json = std::ptr::null_mut();
                        (*resp).error = c_str.into_raw();
                    }
                }
            }
        }

        unsafe extern "C" fn plugin_free_response(resp: *mut $crate::abi::InvokeResponse) {
            unsafe {
                if !(*resp).payload_json.is_null() {
                    let _ = std::ffi::CString::from_raw((*resp).payload_json);
                }
                if !(*resp).error.is_null() {
                    let _ = std::ffi::CString::from_raw((*resp).error);
                }
            }
        }
    };
}
