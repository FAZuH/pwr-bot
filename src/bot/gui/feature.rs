//! The sealed [`GuiFeature`] trait — the shell contract for a TEA command view.

use poise::serenity_prelude::CreateComponent;

use crate::bot::navigation::Navigation;
use crate::bot::view::Action;
use crate::bot::view::ActionRegistry;
use crate::bot::view::SelectValues;
use crate::bot::view::ViewChannelConfig;
use crate::bot::view::ViewEvent;

/// Seal for [`GuiFeature`] — only this module's features may impl it.
pub mod sealed {
    /// Marker trait that closes [`GuiFeature`] to external implementors.
    pub trait Sealed {}
}

/// One interactive command feature's TEA contract.
///
/// An implementor is a shell feature: it owns how the feature's `Model` maps
/// to Discord components (`view`), how a fired action maps back to a `Msg`
/// (`translate`), and where the loop should go once the feature is done
/// (`exit_navigation`). The pure mutation of the model lives in
/// `crate::update::<feature>` and is reached through [`GuiFeature::update`].
///
/// # Invariants
///
/// - `update` is pure — it mutates only the model and returns effects as data.
/// - `view` is a pure `&Model` read — no IO, no mutation.
/// - Effects are data-only and coarse-grained; the adapters in
///   [`crate::bot::gui::effects`] execute them, never the feature.
/// - Implementors are closed — the [`sealed::Sealed`] supertrait makes the
///   closure compiler-enforced: only a type that impls [`sealed::Sealed`] can
///   be a [`GuiFeature`], and only this module carries those impls.
pub trait GuiFeature: sealed::Sealed + Sized + Send + Sync + 'static {
    /// The feature's full state, single source of truth.
    type Model;
    /// Exhaustive message vocabulary: interactions, async results, lifecycle.
    type Msg: Send + 'static;
    /// UI action enum, reusing the shared [`Action`] trait for labels.
    type Action: Action + 'static;
    /// Data-only effect vocabulary.
    type Effect: Send + 'static;
    /// Data-in for construction, supplied by the command handler at boot.
    type Config;

    /// Builds the model from the data-in config.
    fn initial(config: Self::Config) -> Self::Model;

    /// The message the host dispatches at boot to drive the first
    /// update/render cycle.
    fn start_msg() -> Self::Msg;

    /// The message the host dispatches when the loop times out.
    fn timeout_msg() -> Self::Msg;

    /// Pure transition — the only writer of the model. Returns effects as data.
    fn update(msg: Self::Msg, model: &mut Self::Model) -> Vec<Self::Effect>;

    /// Pure `&Model` read → components. Registers actions (custom_ids) into
    /// the registry in the same order and with the same labels as the view's
    /// render code. No IO. The output borrows from the model so views may
    /// read model state directly without copying.
    fn view<'a>(
        model: &'a Self::Model,
        registry: &mut ActionRegistry<Self::Action>,
    ) -> Vec<CreateComponent<'a>>;

    /// Input translation: resolves a fired action (+ select values) into the
    /// `Msg` it means. `None` → unknown id (the host acks and continues).
    fn translate(
        action: &Self::Action,
        values: SelectValues,
        model: &Self::Model,
    ) -> Option<Self::Msg>;

    /// Which collectors the host starts for this feature.
    fn channel_config() -> ViewChannelConfig {
        ViewChannelConfig::default()
    }

    /// When the host finishes processing `msg`, the navigation target it
    /// should apply before ending the loop. `None` → end without navigating
    /// (e.g. a timeout).
    fn exit_navigation(_msg: &Self::Msg) -> Option<Navigation> {
        None
    }

    /// Translates a non-component event (modal/message/reaction/async) into a
    /// `Msg`. `None` → the host acks (if applicable) and continues.
    fn on_event(_event: &ViewEvent, _model: &Self::Model) -> Option<Self::Msg> {
        None
    }
}
