/// Generate the FFI glue for a plugin.
///
/// The plugin type must implement [`BotPlugin`] and be constructible as a
/// `const` expression (e.g. a unit struct or a struct where all fields are
/// `const`-compatible).
#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty, $initializer:expr) => {
        static PLUGIN: $plugin_type = $initializer;

        fn build_metadata_json() -> *mut std::ffi::c_char {
            let meta = $crate::PluginMetadata {
                api_version: $crate::PWR_BOT_PLUGIN_API_VERSION,
                name: PLUGIN.name().to_string(),
                description: PLUGIN.description().to_string(),
                version: PLUGIN.version().to_string(),
                commands: PLUGIN.commands(),
                event_handlers: PLUGIN.event_handlers(),
                settings_panels: PLUGIN.settings_panels(),
                test_steps: PLUGIN.test_steps(),
                tasks: PLUGIN.tasks(),
            };
            let json = serde_json::to_string(&meta).unwrap_or_else(|_| "{}".to_string());
            std::ffi::CString::new(json).unwrap().into_raw()
        }

        #[no_mangle]
        pub unsafe extern "C" fn pwr_bot_plugin_entry() -> *const $crate::abi::PluginVTable {
            static VTABLE: $crate::abi::PluginVTable = $crate::abi::PluginVTable {
                api_version: $crate::abi::PWR_BOT_PLUGIN_API_VERSION,
                metadata: plugin_metadata,
                invoke: plugin_invoke,
                init: Some(plugin_init),
                shutdown: Some(plugin_shutdown),
                on_event: Some(plugin_on_event),
                free_string: Some(plugin_free_string),
            };
            std::ptr::addr_of!(VTABLE)
        }

        unsafe extern "C" fn plugin_metadata() -> *mut std::ffi::c_char {
            build_metadata_json()
        }

        unsafe extern "C" fn plugin_free_string(s: *mut std::ffi::c_char) {
            if !s.is_null() {
                unsafe {
                    let _ = std::ffi::CString::from_raw(s);
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

        unsafe extern "C" fn plugin_init(
            req: *const $crate::abi::InvokeRequest,
            resp: *mut $crate::abi::InvokeResponse,
        ) {
            let req = unsafe { &*req };
            let host = $crate::host::PluginHost::new(req.ctx_handle, unsafe { &*req.callbacks });

            let result =
                tokio::runtime::Handle::current().block_on(async { PLUGIN.init(&host).await });

            match result {
                Ok(()) => unsafe {
                    (*resp).payload_json = std::ptr::null_mut();
                    (*resp).error = std::ptr::null_mut();
                },
                Err(err) => {
                    let c_str = std::ffi::CString::new(err).unwrap();
                    unsafe {
                        (*resp).payload_json = std::ptr::null_mut();
                        (*resp).error = c_str.into_raw();
                    }
                }
            }
        }

        unsafe extern "C" fn plugin_shutdown() -> bool {
            tokio::runtime::Handle::current()
                .block_on(async { PLUGIN.shutdown().await })
                .is_ok()
        }

        unsafe extern "C" fn plugin_on_event(
            event_name: *const std::ffi::c_char,
            payload_json: *const std::ffi::c_char,
            callbacks: *const $crate::abi::HostCallbacks,
            ctx_handle: u64,
        ) -> bool {
            let name = unsafe { std::ffi::CStr::from_ptr(event_name).to_str().unwrap_or("") };
            let payload_str = unsafe {
                std::ffi::CStr::from_ptr(payload_json)
                    .to_str()
                    .unwrap_or("{}")
            };
            let payload: serde_json::Value =
                serde_json::from_str(payload_str).unwrap_or(serde_json::Value::Null);
            let host = $crate::host::PluginHost::new(ctx_handle, unsafe { &*callbacks });

            tokio::runtime::Handle::current()
                .block_on(async { PLUGIN.on_event(name, payload, &host).await })
                .is_ok()
        }
    };
}
