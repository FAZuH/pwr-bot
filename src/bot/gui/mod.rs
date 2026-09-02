//! The TEA GUI shell — replaces [`crate::bot::view::ViewEngine`] for migrated
//! interactive command features.
//!
//! ## One-way dataflow
//!
//! ```text
//!              ┌─────────────────────────────────────────┐
//!              │         Host (this module, `rt.rs`)       │
//!              │  event loop · collectors · ack · timeout  │
//!              └──────┬──────────────────────▲────────────┘
//!     events/tick     │                      │  Effects (data)
//!                     ▼                      │
//!               ┌─────────┐   update     ┌─────────┐
//!   user input  │ Msg     │─────────────►│ Model   │
//!               └─────────┘  (pure)      └────┬────┘
//!                                            │  & (read-only)
//!                                            ▼
//!                                       ┌─────────┐
//!                                       │  view   │
//!                                       └─────────┘
//! ```
//!
//! ## Layer rules
//!
//! - **Pure core** lives in `crate::update/<feature>`: `Model`, `Msg`,
//!   `Effect`, `update`. No serenity/tokio/DB/diesel.
//! - **Shell** is this module: the [`GuiFeature`](feature::GuiFeature)
//!   contract, the [`Host`](rt::Host) runtime, and the pure `view` / input
//!   translation. The shell may touch serenity types, but never does IO in
//!   `view`.
//! - **Adapters** execute effects through the
//!   [`EffectHandler`](effects::EffectHandler) port — the only IO.
//!
//! Implementors of [`GuiFeature`] are closed to this module.

pub mod about;
pub mod effects;
pub mod feature;
pub mod feed_batch;
pub mod feed_list;
pub mod feed_settings;
pub mod input;
pub mod register;
pub mod rt;
pub mod unregister;
pub mod voice_settings;

pub use effects::EffectHandler;
pub use effects::NoopEffectHandler;
pub use feature::GuiFeature;
pub use rt::Host;
