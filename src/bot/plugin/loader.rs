//! Loads plugin `.so` files using `libloading`.

use std::path::Path;
use std::sync::Arc;

use libloading::Library;
use log::error;
use log::info;
use log::warn;
use pwr_bot_sdk::PWR_BOT_PLUGIN_API_VERSION;
use pwr_bot_sdk::PWR_BOT_PLUGIN_ENTRY;
use pwr_bot_sdk::PluginVTable;

/// A loaded plugin instance.
pub struct LoadedPlugin {
    /// Name of the plugin (from vtable).
    pub name: String,
    /// The dynamic library handle.
    _lib: Arc<Library>,
    /// The plugin's virtual table.
    pub vtable: &'static PluginVTable,
}

/// Loads all plugins from a directory.
pub fn load_plugins(dir: &Path) -> Vec<LoadedPlugin> {
    if !dir.exists() {
        info!("Plugin directory does not exist: {dir:?}");
        return vec![];
    }

    let mut plugins = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            error!("Failed to read plugin directory {dir:?}: {e}");
            return vec![];
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if path.extension().and_then(|s| s.to_str()) != Some("so") {
            continue;
        }

        match unsafe { load_plugin(&path) } {
            Ok(plugin) => {
                info!("Loaded plugin: {} (v{:?})", plugin.name, path);
                plugins.push(plugin);
            }
            Err(e) => {
                warn!("Failed to load plugin {path:?}: {e}");
            }
        }
    }

    plugins
}

/// Loads a single plugin `.so` file.
///
/// # Safety
///
/// The caller must ensure that:
/// - The file at `path` is a valid ELF shared library exporting a
///   `pwr_bot_plugin_entry` symbol with the correct signature.
/// - The resulting [`LoadedPlugin`] is not used after the library is unmapped.
///   The library handle is leaked intentionally to guarantee this.
pub unsafe fn load_plugin(path: &Path) -> Result<LoadedPlugin, String> {
    let lib = Arc::new(unsafe { Library::new(path) }.map_err(|e| format!("dlopen failed: {e}"))?);

    let entry_ptr: libloading::Symbol<unsafe extern "C" fn() -> *const PluginVTable> = unsafe {
        lib.get(PWR_BOT_PLUGIN_ENTRY)
            .map_err(|e| format!("symbol 'pwr_bot_plugin_entry' not found: {e}"))?
    };

    let vtable_ptr = unsafe { entry_ptr() };
    if vtable_ptr.is_null() {
        return Err("entry point returned null vtable".to_string());
    }

    let vtable = unsafe { &*vtable_ptr };

    if vtable.api_version != PWR_BOT_PLUGIN_API_VERSION {
        return Err(format!(
            "API version mismatch: plugin requires {}, host provides {}",
            vtable.api_version, PWR_BOT_PLUGIN_API_VERSION
        ));
    }

    let _ = Arc::into_raw(lib.clone());

    Ok(LoadedPlugin {
        _lib: lib,
        vtable,
        name: String::new(),
    })
}
