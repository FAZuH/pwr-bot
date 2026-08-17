//! Errors from the plugin runtime: spawning, handshaking, and calling a
//! plugin subprocess, plus installing plugins from the external catalog.

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

    /// The hello carried a manifest that failed validation, or whose name
    /// does not match the hello's.
    #[error("invalid manifest from plugin `{name}`: {detail}")]
    Manifest {
        /// Plugin name from its hello.
        name: String,
        /// Why the manifest was rejected.
        detail: String,
    },

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

    /// A plugin with the same name is already registered and running.
    #[error("plugin `{name}` is already running")]
    AlreadyRunning {
        /// Plugin name.
        name: String,
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

/// An error installing a plugin from the external catalog: loading the
/// catalog, downloading, verifying, or atomically installing the binary.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// The plugin catalog could not be read or parsed.
    #[error("failed to load plugin catalog `{path}`: {detail}")]
    Catalog {
        /// The catalog file that failed.
        path: PathBuf,
        /// Why the catalog was rejected.
        detail: String,
    },

    /// The plugin binary download failed (network, status, or body read).
    #[error("failed to download plugin `{name}` from `{url}`: {source}")]
    Download {
        /// Plugin name.
        name: String,
        /// The download URL.
        url: String,
        /// The underlying HTTP error.
        #[source]
        source: wreq::Error,
    },

    /// The downloaded (or installed) bytes did not match the pinned sha256.
    #[error("plugin `{name}` sha256 mismatch: expected {expected}, got {got}")]
    Verify {
        /// Plugin name.
        name: String,
        /// The pinned sha256.
        expected: String,
        /// The computed sha256.
        got: String,
    },

    /// The verified binary could not be moved into place.
    #[error("failed to install plugin `{name}` to `{path}`: {source}")]
    Install {
        /// Plugin name.
        name: String,
        /// The install target path.
        path: PathBuf,
        /// The underlying OS error.
        #[source]
        source: io::Error,
    },

    /// The download body exceeded the size cap before it could be verified.
    #[error("plugin `{name}` download from `{url}` is {size} bytes, over the {max} byte cap")]
    TooLarge {
        /// Plugin name.
        name: String,
        /// The download URL.
        url: String,
        /// Bytes received before the cap was hit.
        size: u64,
        /// The size cap the download exceeded.
        max: u64,
    },

    /// The binary's ELF machine type does not match the host architecture.
    #[error("plugin `{name}` is built for {got}, host runs {expected}")]
    ArchMismatch {
        /// Plugin name.
        name: String,
        /// The binary's ELF machine id.
        got: u16,
        /// The host's ELF machine id.
        expected: u16,
    },

    /// The install target is a symlink; writing through it is refused.
    #[error("plugin install target `{path}` is a symlink; refusing to overwrite")]
    Symlink {
        /// The refused target path.
        path: PathBuf,
    },

    /// The downloaded binary is not an ELF executable.
    #[error("plugin `{name}` is not an ELF executable")]
    NotElf {
        /// Plugin name.
        name: String,
    },

    /// The host architecture has no known ELF machine id, so installs are
    /// rejected rather than guessing.
    #[error("host architecture `{arch}` is not supported for plugin installs")]
    UnsupportedHostArch {
        /// The host architecture reported by `std::env::consts::ARCH`.
        arch: String,
    },

    /// I/O failure on a plugin file (temp or installed binary).
    #[error("plugin file io error on `{path}`: {source}")]
    Io {
        /// The file that failed.
        path: PathBuf,
        /// The underlying OS error.
        #[source]
        source: io::Error,
    },
}
