//! Pure update logic for the `/register` command.
//!
//! Holds the single source of truth for the command registration status view
//! (`RegisterModel`) and the exhaustive message vocabulary (`RegisterMsg`).
//! Command registration is a one-shot shell concern (the handler performs the
//! actual `guild_id.set_commands` call); this core tracks the status and
//! renders it.

/// The command registration status view model — the single source of truth.
#[derive(Debug, Clone)]
pub struct RegisterModel {
    /// Number of commands being registered.
    pub num_commands: usize,
    /// Whether registration is complete.
    pub is_complete: bool,
    /// Time taken in milliseconds (if complete).
    pub duration_ms: Option<u64>,
}

impl RegisterModel {
    /// Constructs the initial registration model.
    pub fn new(num_commands: usize) -> Self {
        Self {
            num_commands,
            is_complete: false,
            duration_ms: None,
        }
    }
}

/// Messages that drive the registration view.
///
/// Exhaustive: every way the world can change the registration model is one
/// variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterMsg {
    /// Boot handshake — the host dispatches this on start.
    Start,
    /// The view loop timed out.
    Expired,
    /// Command registration finished.
    Registered { duration_ms: u64 },
}

/// Effects the registration view can request.
///
/// Empty: command registration is a one-shot shell concern executed by the
/// command handler; this core only tracks and renders the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterEffect {}

/// The pure update function — the only writer of the model.
pub fn update(msg: RegisterMsg, model: &mut RegisterModel) -> Vec<RegisterEffect> {
    match msg {
        RegisterMsg::Registered { duration_ms } => {
            model.is_complete = true;
            model.duration_ms = Some(duration_ms);
        }
        RegisterMsg::Start | RegisterMsg::Expired => {}
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_keeps_model_incomplete() {
        let mut m = RegisterModel::new(5);
        let effects = update(RegisterMsg::Start, &mut m);
        assert!(effects.is_empty());
        assert!(!m.is_complete);
        assert_eq!(m.duration_ms, None);
    }

    #[test]
    fn registered_marks_complete_with_duration() {
        let mut m = RegisterModel::new(5);
        let effects = update(RegisterMsg::Registered { duration_ms: 1234 }, &mut m);
        assert!(effects.is_empty());
        assert!(m.is_complete);
        assert_eq!(m.duration_ms, Some(1234));
    }
}
