//! Feed list subcommand.

use std::collections::HashSet;
use std::time::Duration;

use serenity::all::ButtonStyle;
use serenity::all::ComponentInteraction;
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
use crate::bot::views::ChildViewResolver;
use crate::bot::views::InteractiveView;
use crate::bot::views::InteractiveViewBase;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::bot::views::ViewCommand;
use crate::bot::views::ViewCore;
use crate::bot::views::ViewHandler;
use crate::bot::views::pagination::PaginationAction;
use crate::bot::views::pagination::PaginationView;
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

        let pagination = PaginationView::new(&ctx, total_items, SUBSCRIPTIONS_PER_PAGE);
        let mut view = FeedListView::new(
            &ctx,
            subscriptions,
            pagination,
            service.clone(),
            subscriber.clone(),
        );
        let mut exit_nav = NavigationResult::Exit;

        view.run(|action| {
            let _nav_ref = &mut exit_nav;
            Box::pin(async move {
                match action {
                    FeedListAction::Exit => ViewCommand::Exit,
                    _ => ViewCommand::Render,
                }
            })
        })
        .await?;

        // Handle the save logic after the view loop completes
        if view.handler.state == FeedListState::View && !view.handler.marked_unsub.is_empty() {
            for url in &view.handler.marked_unsub {
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

pub struct FeedListHandler<'a> {
    pub subscriptions: Vec<Subscription>,
    pub pagination: PaginationView<'a>,
    pub state: FeedListState,
    pub marked_unsub: HashSet<String>,
    pub service: std::sync::Arc<crate::service::feed_subscription_service::FeedSubscriptionService>,
    pub subscriber: crate::model::SubscriberModel,
    pub ctx_ref: &'a Context<'a>,
}

pub struct FeedListView<'a> {
    pub base: InteractiveViewBase<'a, FeedListAction>,
    pub handler: FeedListHandler<'a>,
}

impl<'a> View<'a, FeedListAction> for FeedListView<'a> {
    fn core(&self) -> &ViewCore<'a, FeedListAction> {
        &self.base.core
    }
    fn core_mut(&mut self) -> &mut ViewCore<'a, FeedListAction> {
        &mut self.base.core
    }
    fn create_core(poise_ctx: &'a Context<'a>) -> ViewCore<'a, FeedListAction> {
        ViewCore::new(poise_ctx, Duration::from_secs(120))
    }
}

impl<'a> FeedListView<'a> {
    pub fn new(
        ctx: &'a Context<'a>,
        subscriptions: Vec<Subscription>,
        pagination: PaginationView<'a>,
        service: std::sync::Arc<crate::service::feed_subscription_service::FeedSubscriptionService>,
        subscriber: crate::model::SubscriberModel,
    ) -> Self {
        Self {
            base: InteractiveViewBase::new(Self::create_core(ctx)),
            handler: FeedListHandler {
                subscriptions,
                pagination,
                state: FeedListState::View,
                marked_unsub: HashSet::new(),
                service,
                subscriber,
                ctx_ref: ctx,
            },
        }
    }

    /// Updates the subscriptions list.
    pub fn set_subscriptions(&mut self, subscriptions: Vec<Subscription>) -> &mut Self {
        self.handler.subscriptions = subscriptions;
        self
    }

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
        &mut self,
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

        let accessory = match self.handler.state {
            FeedListState::View => CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                CreateUnfurledMediaItem::new(sub.feed.cover_url),
            )),
            FeedListState::Edit => {
                let source_url = sub.feed.source_url;
                let button = if self.handler.marked_unsub.contains(&source_url) {
                    self.base
                        .register(UndoUnsub { source_url })
                        .as_button()
                        .style(ButtonStyle::Secondary)
                } else {
                    self.base
                        .register(Unsubscribe { source_url })
                        .as_button()
                        .style(ButtonStyle::Danger)
                };
                CreateSectionAccessory::Button(button)
            }
        };

        CreateContainerComponent::Section(CreateSection::new(vec![text_component], accessory))
    }

    /// Create button section of the view at the bottom.
    fn create_toggle_button<'b>(&mut self) -> CreateComponent<'b> {
        let action = match self.handler.state {
            FeedListState::Edit => FeedListAction::View,
            FeedListState::View => FeedListAction::Edit,
        };

        let state_button = self
            .base
            .register(action)
            .as_button()
            .style(ButtonStyle::Primary);

        let mut save_button = self
            .base
            .register(FeedListAction::Save)
            .as_button()
            .style(ButtonStyle::Success);

        if self.handler.marked_unsub.is_empty() {
            save_button = save_button.disabled(true)
        }

        let buttons = vec![state_button, save_button];

        CreateComponent::ActionRow(CreateActionRow::Buttons(buttons.into()))
    }
}

impl<'a> ResponseView<'a> for FeedListView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        if self.handler.subscriptions.is_empty() {
            return Self::create_empty().into();
        }

        let sections: Vec<CreateContainerComponent<'b>> = self
            .handler
            .subscriptions
            .clone()
            .into_iter()
            .map(|sub| self.create_subscription_section(sub))
            .collect();

        let container = CreateComponent::Container(CreateContainer::new(sections));
        let mut components = vec![container];

        self.handler.pagination.attach_if_multipage(&mut components);
        components.push(self.create_toggle_button());

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
impl<'a> ViewHandler<FeedListAction> for FeedListHandler<'a> {
    async fn handle(
        &mut self,
        action: &FeedListAction,
        interaction: &ComponentInteraction,
    ) -> Option<FeedListAction> {
        use FeedListAction::*;
        match action {
            Base(pagination_action) => {
                let action = self
                    .pagination
                    .handler
                    .handle(pagination_action, interaction)
                    .await?;

                // Refresh page
                if let Ok(subs) = self
                    .service
                    .list_paginated_subscriptions(
                        &self.subscriber,
                        self.pagination.handler.state.current_page,
                        SUBSCRIPTIONS_PER_PAGE,
                    )
                    .await
                {
                    self.subscriptions = subs;
                }

                Some(Base(action))
            }
            Edit => {
                self.state = FeedListState::Edit;
                Some(action.clone())
            }
            View => {
                self.state = FeedListState::View;
                Some(action.clone())
            }
            Unsubscribe { source_url } => {
                self.marked_unsub.insert(source_url.clone());
                Some(action.clone())
            }
            UndoUnsub { source_url } => {
                self.marked_unsub.remove(source_url);
                Some(action.clone())
            }
            Exit => Some(action.clone()),
            Save => {
                // Controller handles the actual DB unsubscribe
                self.state = FeedListState::View;
                Some(action.clone())
            }
        }
    }

    async fn on_timeout(&mut self) -> Result<(), Error> {
        self.pagination.handler.disabled = true;
        Ok(())
    }

    fn children(&mut self) -> Vec<Box<dyn ChildViewResolver<FeedListAction> + '_>> {
        vec![crate::bot::views::child(
            &mut self.pagination,
            FeedListAction::Base,
        )]
    }
}

crate::impl_interactive_view!(FeedListView<'a>, FeedListHandler<'a>, FeedListAction);
