//! Views for admin commands using Discord Components V2.

use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateTextDisplay;

use crate::bot::views::ResponseComponentView;

/// View for command registration status.
pub struct CommandRegistrationView {
    /// Number of commands being registered
    num_commands: usize,
    /// Whether registration is complete
    is_complete: bool,
    /// Time taken in milliseconds (if complete)
    duration_ms: Option<u64>,
}

impl CommandRegistrationView {
    /// Creates a new registration view.
    pub fn new(num_commands: usize) -> Self {
        Self {
            num_commands,
            is_complete: false,
            duration_ms: None,
        }
    }

    /// Marks the registration as complete with duration.
    pub fn complete(mut self, duration_ms: u64) -> Self {
        self.is_complete = true;
        self.duration_ms = Some(duration_ms);
        self
    }
}

impl ResponseComponentView for CommandRegistrationView {
    fn create_components<'a>(&self) -> Vec<CreateComponent<'a>> {
        let title = if self.is_complete {
            "Command Registration Complete"
        } else {
            "Registering Commands"
        };

        let status_text = if self.is_complete {
            format!(
                "### {}\nSuccessfully registered {} commands in {}ms",
                title,
                self.num_commands,
                self.duration_ms.unwrap_or(0)
            )
        } else {
            format!(
                "### {}\nRegistering {} guild commands...",
                title, self.num_commands
            )
        };

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
        ]));

        vec![container]
    }
}

/// View for command unregistration status.
pub struct CommandUnregistrationView {
    /// Whether unregistration is complete
    is_complete: bool,
    /// Time taken in milliseconds (if complete)
    duration_ms: Option<u64>,
}

impl CommandUnregistrationView {
    /// Creates a new unregistration view.
    pub fn new() -> Self {
        Self {
            is_complete: false,
            duration_ms: None,
        }
    }

    /// Marks the unregistration as complete with duration.
    pub fn complete(mut self, duration_ms: u64) -> Self {
        self.is_complete = true;
        self.duration_ms = Some(duration_ms);
        self
    }
}

impl ResponseComponentView for CommandUnregistrationView {
    fn create_components<'a>(&self) -> Vec<CreateComponent<'a>> {
        let title = if self.is_complete {
            "Command Unregistration Complete"
        } else {
            "Unregistering Commands"
        };

        let status_text = if self.is_complete {
            format!(
                "### {}\nSuccessfully unregistered all commands in {}ms",
                title,
                self.duration_ms.unwrap_or(0)
            )
        } else {
            format!("### {}\nUnregistering all guild commands...", title)
        };

        let container = CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
        ]));

        vec![container]
    }
}
