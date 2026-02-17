//! Settings-related views and coordinator shared across command modules.
//!
//! This module provides the settings coordinator and reusable view components
//! for settings interfaces.

use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::CreateActionRow;
use serenity::all::CreateButton;
use serenity::all::CreateComponent;

use crate::action_enum;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::about::AboutController;
use crate::bot::commands::admin::controllers::SettingsMainController;
use crate::bot::commands::feed::controllers::FeedSettingsController;
use crate::bot::commands::voice::controllers::VoiceSettingsController;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::Action;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::view_core;

view_core! {
    timeout = Duration::from_secs(120),
    /// Navigation bar for settings views.
    ///
    /// Provides a consistent navigation bar with optional back button,
    /// about button, and help button (placeholder).
    pub struct SettingsNavigationView<'a, SettingsNavAction> {
        /// Whether to show the back button
        show_back: bool,
        /// Whether to show the help button (currently disabled)
        #[allow(dead_code)]
        show_help: bool,
    }
}

impl<'a> SettingsNavigationView<'a> {
    /// Creates a new navigation view for the main settings page.
    pub fn main(ctx: &'a Context<'a>) -> Self {
        Self {
            show_back: false,
            show_help: false,
            core: Self::create_core(ctx),
        }
    }
}

action_enum! {
    SettingsNavAction {
        /// Navigate back to the parent settings page
        #[label = "< Back"]
        Back,
        /// Show information about the bot
        #[label = "🛈 About"]
        About,
        /// Show help (not yet implemented)
        #[label = "Help"]
        Help,
    }
}

impl<'a> ResponseView<'a> for SettingsNavigationView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        let mut buttons = Vec::new();

        if self.show_back {
            buttons.push(
                CreateButton::new(self.register(SettingsNavAction::Back))
                    .label(SettingsNavAction::Back.label())
                    .style(ButtonStyle::Secondary),
            );
        }

        // Help button placeholder - commented out until /help command is implemented
        // if self.show_help {
        //     buttons.push(
        //         CreateButton::new(SettingsNavAction::Help.custom_id())
        //             .label(SettingsNavAction::Help.label())
        //             .style(ButtonStyle::Secondary),
        //     );
        // }

        buttons.push(
            CreateButton::new(self.register(SettingsNavAction::About))
                .label(SettingsNavAction::About.label())
                .style(ButtonStyle::Secondary),
        );

        vec![CreateComponent::ActionRow(CreateActionRow::Buttons(
            buttons.into(),
        ))]
        .into()
    }
}

/// Tracks the current settings page for navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    /// Main settings page
    Main,
    /// Feed settings page
    Feeds,
    /// Voice settings page
    Voice,
    /// About page
    About,
}

/// Runs the settings coordinator loop.
///
/// This is the entry point for the settings command. It creates a coordinator
/// and runs controllers in a loop based on their NavigationResult.
///
/// # Parameters
///
/// - `ctx`: The Discord command context
/// - `current_page`: Initial page to show. If None, defaults to [`SettingsPage::Main`]
pub async fn run_settings(
    ctx: Context<'_>,
    initial_page: Option<SettingsPage>,
) -> Result<(), Error> {
    let mut coordinator = Coordinator::new(ctx);
    let mut current_page = initial_page.unwrap_or(SettingsPage::Main);

    loop {
        // Create controller based on current page
        // Clone the context to avoid borrow checker issues
        let ctx_clone = *coordinator.context();
        let result = match current_page {
            SettingsPage::Main => {
                let mut controller = SettingsMainController::new(&ctx_clone);
                controller.run(&mut coordinator).await?
            }
            SettingsPage::Feeds => {
                let mut controller = FeedSettingsController::new(&ctx_clone);
                controller.run(&mut coordinator).await?
            }
            SettingsPage::Voice => {
                let mut controller = VoiceSettingsController::new(&ctx_clone);
                controller.run(&mut coordinator).await?
            }
            SettingsPage::About => {
                let mut controller = AboutController::new(&ctx_clone);
                controller.run(&mut coordinator).await?
            }
        };

        // Update current page based on navigation result
        match result {
            NavigationResult::SettingsMain => current_page = SettingsPage::Main,
            NavigationResult::SettingsFeeds => current_page = SettingsPage::Feeds,
            NavigationResult::SettingsVoice => current_page = SettingsPage::Voice,
            NavigationResult::SettingsAbout => current_page = SettingsPage::About,
            NavigationResult::Back => {
                // Go back to main from any sub-page
                current_page = SettingsPage::Main;
            }
            NavigationResult::Exit => break,
            // Other navigation variants not valid from settings
            _ => continue,
        }
    }

    Ok(())
}
