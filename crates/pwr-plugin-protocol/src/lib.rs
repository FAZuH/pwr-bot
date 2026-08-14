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

pub mod msg;

pub use msg::API_VERSION;
pub use msg::CallIdSeq;
pub use msg::Msg;
pub use msg::WireError;
