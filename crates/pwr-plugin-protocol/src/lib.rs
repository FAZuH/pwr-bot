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
//! only, stderr is the free logging channel. See the `hello` plugin
//! (`crates/plugin/hello/src/main.rs`) for the reference implementation.

pub mod manifest;
pub mod msg;
pub mod ops;
pub mod settings;
pub mod stats;
pub mod view;

pub use manifest::ALL_REQUIREMENTS;
pub use manifest::CommandDef;
pub use manifest::DISCORD_TOKEN;
pub use manifest::Manifest;
pub use manifest::SettingsSection;
pub use manifest::TaskDef;
pub use msg::API_VERSION;
pub use msg::BUTTON_CUSTOM_ID;
pub use msg::CallIdSeq;
pub use msg::MODAL_OPENED_KIND;
pub use msg::MODAL_SUBMIT_OP;
pub use msg::Msg;
pub use msg::PLUGIN_NAME;
pub use msg::VIEW_MOVED_KIND;
pub use msg::WireError;
pub use ops::ALL_OPS;
pub use ops::HostOp;
pub use ops::OpsError;
pub use ops::ResolvedUser;
pub use ops::validate_ops;

/// The host-reserved `host.open_view` target that hands a panel's message
/// back to the host Settings GUI. Not a plugin name: the host answers the
/// target itself, by waking the session that handed the message to the
/// panel.
pub const SETTINGS_TARGET: &str = "settings";

/// The host-reserved `host.open_view` target that opens the host About view
/// on the panel's message, waking the session that handed it to the panel.
/// Like [`SETTINGS_TARGET`], not a plugin name: the host answers the target
/// itself.
pub const ABOUT_TARGET: &str = "about";
pub use settings::FeedsSettings;
pub use settings::ServerSettings;
pub use settings::VoiceSettings;
pub use settings::WelcomeSettings;
pub use stats::HostStats;
pub use view::RuntimeFile;
pub use view::ViewPayload;
pub use view::ViewPayloadError;
pub use view::ViewSpec;
pub use view::view_payload;
