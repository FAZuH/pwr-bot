/// Generate the FFI glue for a plugin.
///
/// This macro generates the `extern "C"` functions required by the plugin ABI,
/// including the entry point ([`PWR_BOT_PLUGIN_ENTRY`](crate::PWR_BOT_PLUGIN_ENTRY)),
/// metadata, invoke, init, shutdown, and event dispatch.
///
/// The plugin type must implement [`BotPlugin`](crate::plugin::BotPlugin).
///
/// # Usage
///
/// ```ignore
/// use pwr_bot_sdk::export_plugin;
///
/// struct MyPlugin;
/// // ... implement BotPlugin for MyPlugin ...
///
/// export_plugin!(MyPlugin, MyPlugin);
/// ```
///
/// The second argument is an expression that constructs the plugin instance.
/// It is called once and cached in a `OnceLock`.
#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty, $initializer:expr) => {
        use ::std::sync::OnceLock;

        static PLUGIN_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

        fn plugin_runtime() -> &'static tokio::runtime::Runtime {
            PLUGIN_RUNTIME.get_or_init(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create plugin tokio runtime")
            })
        }

        fn plugin_instance() -> &'static $plugin_type {
            static PLUGIN: OnceLock<$plugin_type> = OnceLock::new();
            PLUGIN.get_or_init(|| $initializer)
        }

        fn build_metadata_json() -> *mut std::ffi::c_char {
            let plugin = plugin_instance();
            let meta = $crate::PluginMetadata {
                api_version: $crate::PWR_BOT_PLUGIN_API_VERSION,
                name: plugin.name().to_string(),
                description: plugin.description().to_string(),
                version: plugin.version().to_string(),
                commands: plugin.commands(),
                event_handlers: plugin.event_handlers(),
                settings_panels: plugin.settings_panels(),
                test_steps: plugin.test_steps(),
                tasks: plugin.tasks(),
            };
            let json = serde_json::to_string(&meta).unwrap_or_else(|_| "{}".to_string());
            std::ffi::CString::new(json).unwrap().into_raw()
        }

        #[unsafe(no_mangle)]
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
            let plugin = plugin_instance();
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

            let result = plugin_runtime().block_on(async { plugin.invoke(&command, args, &host).await });

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
            let plugin = plugin_instance();
            let req = unsafe { &*req };
            let host = $crate::host::PluginHost::new(req.ctx_handle, unsafe { &*req.callbacks });

            let result = plugin_runtime().block_on(async { plugin.init(&host).await });

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
            let plugin = plugin_instance();
            plugin_runtime().block_on(async { plugin.shutdown().await }).is_ok()
        }

        unsafe extern "C" fn plugin_on_event(
            event_name: *const std::ffi::c_char,
            payload_json: *const std::ffi::c_char,
            callbacks: *const $crate::abi::HostCallbacks,
            ctx_handle: u64,
        ) -> bool {
            let plugin = plugin_instance();
            let name = unsafe { std::ffi::CStr::from_ptr(event_name).to_str().unwrap_or("") };
            let payload_str = unsafe {
                std::ffi::CStr::from_ptr(payload_json)
                    .to_str()
                    .unwrap_or("{}")
            };
            let payload: serde_json::Value =
                serde_json::from_str(payload_str).unwrap_or(serde_json::Value::Null);
            let host = $crate::host::PluginHost::new(ctx_handle, unsafe { &*callbacks });

            plugin_runtime().block_on(async { plugin.on_event(name, payload, &host).await }).is_ok()
        }
    };
}
