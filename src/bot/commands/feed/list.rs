//! Feed list subcommand.

use std::collections::HashSet;
use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::CreateActionRow;
use serenity::all::CreateComponent;
use serenity::all::CreateContainer;
use serenity::all::CreateContainerComponent;
use serenity::all::CreateSection;
use serenity::all::CreateSectionAccessory;
use serenity::all::CreateSectionComponent;
use serenity::all::CreateTextDisplay;
use serenity::all::CreateThumbnail;
use serenity::all::CreateUnfurledMediaItem;

use crate::action_extends;
use crate::bot::commands::Context;
use crate::bot::commands::Error;
use crate::bot::commands::feed::SendInto;
use crate::bot::commands::feed::get_or_create_subscriber;
use crate::bot::controller::Controller;
use crate::bot::controller::Coordinator;
use crate::bot::navigation::NavigationResult;
use crate::bot::views::ActionRegistry;
use crate::bot::views::ResponseKind;
use crate::bot::views::Trigger;
use crate::bot::views::ViewCommand;
use crate::bot::views::ViewContextV2;
use crate::bot::views::ViewEngine;
use crate::bot::views::ViewHandlerV2;
use crate::bot::views::ViewRenderV2;
use crate::bot::views::pagination::PaginationAction;
use crate::bot::views::pagination::PaginationViewV2;
use crate::controller;
use crate::service::feed_subscription_service::Subscription;

/// Number of items per page for subscriptions list.
const SUBSCRIPTIONS_PER_PAGE: u32 = 10;

/// List your current feed subscriptions
///
/// View all feeds you are subscribed to, with pagination support.
#[poise::command(slash_command)]
pub async fn list(
    ctx: Context<'_>,
    #[description = "Where the notifications are being sent. Default to DM"] sent_into: Option<
        SendInto,
    >,
) -> Result<(), Error> {
    command(ctx, sent_into).await
}

pub async fn command(ctx: Context<'_>, sent_into: Option<SendInto>) -> Result<(), Error> {
    let sent_into = sent_into.unwrap_or(SendInto::DM);
    let mut coordinator = Coordinator::new(ctx);
    let mut controller = FeedListController::new(&ctx, sent_into);
    let _ = controller.run(&mut coordinator).await?;
    Ok(())
}

controller! { pub struct FeedListController<'a> {
    send_into: SendInto
} }

#[async_trait::async_trait]
impl<'a, S: Send + Sync + 'static> Controller<S> for FeedListController<'a> {
    async fn run(
        &mut self,
        coordinator: &mut Coordinator<'_, S>,
    ) -> Result<NavigationResult, Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let subscriber = get_or_create_subscriber(ctx, &self.send_into).await?;

        let service = ctx.data().service.feed_subscription.clone();

        let total_items = service.get_subscription_count(&subscriber).await?;

        let subscriptions = service
            .list_paginated_subscriptions(&subscriber, 1u32, SUBSCRIPTIONS_PER_PAGE)
            .await?;

        let pagination = PaginationViewV2::new(total_items, SUBSCRIPTIONS_PER_PAGE);

        let view = FeedListHandler {
            subscriptions,
            pagination,
            state: FeedListState::View,
            marked_unsub: HashSet::new(),
            service: service.clone(),
            subscriber: subscriber.clone(),
        };

        let mut engine = ViewEngine::new(&ctx, view, Duration::from_secs(120));

        let mut exit_nav = NavigationResult::Exit;

        engine
            .run(|action| {
                let _nav_ref = &mut exit_nav;
                Box::pin(async move {
                    match action {
                        FeedListAction::Exit => ViewCommand::Exit,
                        _ => ViewCommand::Render,
                    }
                })
            })
            .await?;

        // Extract handler to save state
        let final_handler = engine.handler;

        // Handle the save logic after the view loop completes
        if final_handler.state == FeedListState::View && !final_handler.marked_unsub.is_empty() {
            for url in &final_handler.marked_unsub {
                service.unsubscribe(url, &subscriber).await.ok();
            }
        }

        Ok(exit_nav)
    }
}

#[derive(PartialEq)]
pub enum FeedListState {
    View,
    Edit,
}

pub struct FeedListHandler {
    pub subscriptions: Vec<Subscription>,
    pub pagination: PaginationViewV2,
    pub state: FeedListState,
    pub marked_unsub: HashSet<String>,
    pub service: std::sync::Arc<crate::service::feed_subscription_service::FeedSubscriptionService>,
    pub subscriber: crate::entity::SubscriberEntity,
}

impl FeedListHandler {
    /// Creates an empty state view.
    fn create_empty<'b>() -> Vec<CreateComponent<'b>> {
        vec![CreateComponent::Container(CreateContainer::new(vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                "You have no subscriptions.",
            )),
        ]))]
    }

    /// Creates a section component for a single subscription.
    fn create_subscription_section<'b>(
        &self,
        registry: &mut ActionRegistry<FeedListAction>,
        sub: Subscription,
    ) -> CreateContainerComponent<'b> {
        use FeedListAction::*;
        let text = if let Some(latest) = sub.feed_latest {
            format!(
                "### {}\n\n- **Last version**: {}\n- **Last updated**: <t:{}>\n- [**Source** 🗗](<{}>)",
                sub.feed.name,
                latest.description,
                latest.published.timestamp(),
                sub.feed.source_url
            )
        } else {
            format!(
                "### {}\n\n> No latest version found.\n- [**Source** 🗗](<{}>)",
                sub.feed.name, sub.feed.source_url
            )
        };

        let text_component = CreateSectionComponent::TextDisplay(CreateTextDisplay::new(text));

        let accessory = match self.state {
            FeedListState::View => CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                CreateUnfurledMediaItem::new(sub.feed.cover_url),
            )),
            FeedListState::Edit => {
                let source_url = sub.feed.source_url;
                let button = if self.marked_unsub.contains(&source_url) {
                    let action = crate::bot::views::RegisteredAction {
                        id: registry.register(UndoUnsub { source_url }),
                        label: "↶ Undo",
                    };
                    action.as_button().style(ButtonStyle::Secondary)
                } else {
                    let action = crate::bot::views::RegisteredAction {
                        id: registry.register(Unsubscribe { source_url }),
                        label: "🗑 Unsubscribe",
                    };
                    action.as_button().style(ButtonStyle::Danger)
                };
                CreateSectionAccessory::Button(button)
            }
        };

        CreateContainerComponent::Section(CreateSection::new(vec![text_component], accessory))
    }

    /// Create button section of the view at the bottom.
    fn create_toggle_button<'b>(
        &self,
        registry: &mut ActionRegistry<FeedListAction>,
    ) -> CreateComponent<'b> {
        let action = match self.state {
            FeedListState::Edit => FeedListAction::View,
            FeedListState::View => FeedListAction::Edit,
        };

        let state_action = crate::bot::views::RegisteredAction {
            id: registry.register(action.clone()),
            label: crate::bot::views::Action::label(&action),
        };
        let state_button = state_action.as_button().style(ButtonStyle::Primary);

        let save_action = crate::bot::views::RegisteredAction {
            id: registry.register(FeedListAction::Save),
            label: "Save",
        };
        let mut save_button = save_action.as_button().style(ButtonStyle::Success);

        if self.marked_unsub.is_empty() {
            save_button = save_button.disabled(true)
        }

        let buttons = vec![state_button, save_button];

        CreateComponent::ActionRow(CreateActionRow::Buttons(buttons.into()))
    }
}

impl ViewRenderV2<FeedListAction> for FeedListHandler {
    fn render(&self, registry: &mut ActionRegistry<FeedListAction>) -> ResponseKind<'_> {
        if self.subscriptions.is_empty() {
            return FeedListHandler::create_empty().into();
        }

        let sections: Vec<CreateContainerComponent<'_>> = self
            .subscriptions
            .clone()
            .into_iter()
            .map(|sub| self.create_subscription_section(registry, sub))
            .collect();

        let container = CreateComponent::Container(CreateContainer::new(sections));
        let mut components = vec![container];

        self.pagination
            .attach_if_multipage(registry, &mut components, FeedListAction::Base);
        components.push(self.create_toggle_button(registry));

        components.into()
    }
}

action_extends! { FeedListAction extends PaginationAction {
    #[label =  "✎ Edit"]
    Edit,
    #[label = "👁 View"]
    View,
    #[label = "🗑 Unsubscribe"]
    Unsubscribe { source_url: String },
    #[label = "↶ Undo"]
    UndoUnsub { source_url: String },
    Save,
    Exit,
}}

#[async_trait::async_trait]
impl ViewHandlerV2<FeedListAction> for FeedListHandler {
    async fn handle(
        &mut self,
        action: FeedListAction,
        _trigger: Trigger<'_>,
        _ctx: &ViewContextV2<'_, FeedListAction>,
    ) -> Result<ViewCommand, Error> {
        use FeedListAction::*;
        match &action {
            Base(_) => {
                // We handle pagination logic below
            }
            Edit => {
                self.state = FeedListState::Edit;
            }
            View => {
                self.state = FeedListState::View;
            }
            Unsubscribe { source_url } => {
                self.marked_unsub.insert(source_url.clone());
            }
            UndoUnsub { source_url } => {
                self.marked_unsub.remove(source_url);
            }
            Exit => return Ok(ViewCommand::Exit),
            Save => {
                // Controller handles the actual DB unsubscribe
                self.state = FeedListState::View;
            }
        };

        if let Base(pagination_action) = action {
            // Re-implement the sub-call manually since ViewHandlerV2 requires the exact generic
            match pagination_action {
                PaginationAction::First => self.pagination.state.first_page(),
                PaginationAction::Prev => self.pagination.state.prev_page(),
                PaginationAction::Next => self.pagination.state.next_page(),
                PaginationAction::Last => self.pagination.state.last_page(),
                _ => return Ok(ViewCommand::Ignore),
            }

            // Refresh page
            if let Ok(subs) = self
                .service
                .list_paginated_subscriptions(
                    &self.subscriber,
                    self.pagination.state.current_page,
                    SUBSCRIPTIONS_PER_PAGE,
                )
                .await
            {
                self.subscriptions = subs;
            }
        }

        Ok(ViewCommand::Render)
    }

    async fn on_timeout(&mut self) -> Result<(), Error> {
        self.pagination.disabled = true;
        Ok(())
    }
}
