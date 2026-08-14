//! Errors from the plugin runtime: spawning, handshaking, and calling a
//! plugin subprocess.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use pwr_plugin_protocol::CapsError;

/// An error from the plugin runtime: spawning a plugin subprocess,
/// handshaking with it, or exchanging calls over the wire.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// The plugin binary could not be spawned.
    #[error("failed to spawn plugin binary `{path}`: {source}")]
    Spawn {
        /// The binary path that failed to spawn.
        path: PathBuf,
        /// The underlying OS error.
        #[source]
        source: io::Error,
    },

    /// The plugin never announced its `hello` within the handshake timeout.
    #[error("plugin `{name}` did not send hello within {timeout:?}")]
    HelloTimeout {
        /// Plugin label (binary file stem) used for logging.
        name: String,
        /// How long the host waited.
        timeout: Duration,
    },

    /// The plugin's first line was not a usable `hello`: bad json, stdout
    /// closed before any line, or a different message.
    #[error("plugin `{name}` did not handshake: {detail}")]
    HelloLost {
        /// Plugin label used for logging.
        name: String,
        /// Why the hello was not accepted.
        detail: String,
    },

    /// The hello announced a wire protocol version the host does not speak.
    #[error("plugin `{name}` speaks protocol version {got}, host speaks {expected}")]
    VersionMismatch {
        /// Plugin name from its hello.
        name: String,
        /// Version the plugin announced.
        got: u32,
        /// Version the host speaks.
        expected: u32,
    },

    /// The hello declared a `host.*` capability the host does not serve.
    #[error(transparent)]
    Caps(#[from] CapsError),

    /// A call was not answered within the call timeout.
    #[error("call `{op}` to plugin `{name}` timed out after {timeout:?}")]
    CallTimeout {
        /// Plugin name.
        name: String,
        /// The operation that timed out.
        op: String,
        /// How long the host waited.
        timeout: Duration,
    },

    /// The plugin is not running, so the call could not be sent.
    #[error("plugin `{name}` is not running")]
    NotRunning {
        /// Plugin name.
        name: String,
    },

    /// The call channel closed without a response — the plugin died.
    #[error("plugin `{name}` died")]
    PluginDied {
        /// Plugin name.
        name: String,
    },

    /// The plugin did not exit within the stop grace period after `bye` and a
    /// kill attempt.
    #[error("plugin `{name}` did not exit within {timeout:?} after stop")]
    StopTimeout {
        /// Plugin name.
        name: String,
        /// How long the host waited for the plugin to exit.
        timeout: Duration,
    },

    /// I/O failure on the plugin's pipes (e.g. broken pipe after death).
    #[error("plugin `{name}` io error: {source}")]
    Io {
        /// Plugin name.
        name: String,
        /// The underlying OS error.
        #[source]
        source: io::Error,
    },
}
