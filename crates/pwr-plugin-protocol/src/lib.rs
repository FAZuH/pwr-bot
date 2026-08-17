//! Wire protocol types for pwr-bot host↔plugin communication.
//!
//! Plugins are standalone executables spawned by the host. Both sides exchange
//! messages as JSON-Lines over stdio: one compact JSON object per line,
//! terminated by a single `\n`. Each message is a [`Msg`] tagged with a `t`
//! discriminator; objects are self-delimiting, so decoding with
//! `serde_json::Deserializer::from_reader` needs no length prefix or framing.
//! stdout carries only protocol lines; stderr is the free logging channel.
//!
//! Envelope variants: `hello` (plugin handshake, the first line after spawn),
//! `call`/`resp` (requests and their answers, correlated by `id`), `event`
//! (one-way push, never answered), `ping`/`pong` (liveness), and `bye`
//! (graceful shutdown). Errors are first-class wire values: a failed `resp`
//! carries `ok:false` plus `error:{kind,msg}`. Panics never cross the wire.
//!
//! Plugin authoring: wrap `main` in `std::panic::catch_unwind`, print the
//! panic payload to stderr, and exit nonzero; stdout carries protocol lines
//! only, stderr is the free logging channel. See the `hello_plugin` fixture
//! (`crates/pwr-plugin-protocol/src/bin/hello_plugin.rs`) for the reference
//! implementation.

pub mod caps;
pub mod manifest;
pub mod msg;
pub mod view;

pub use caps::ALL_CAPS;
pub use caps::CapsError;
pub use caps::HostCap;
pub use caps::validate_caps;
pub use manifest::CommandDef;
pub use manifest::Manifest;
pub use manifest::ManifestError;
pub use manifest::PanelDef;
pub use manifest::TaskDef;
pub use msg::API_VERSION;
pub use msg::BUTTON_CUSTOM_ID;
pub use msg::CallIdSeq;
pub use msg::Msg;
pub use msg::PLUGIN_NAME;
pub use msg::WireError;
pub use view::ViewSpec;
