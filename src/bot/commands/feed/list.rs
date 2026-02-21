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
use crate::bot::views::RenderExt;
use crate::bot::views::ResponseKind;
use crate::bot::views::ResponseView;
use crate::bot::views::View;
use crate::bot::views::pagination::PaginationAction;
use crate::bot::views::pagination::PaginationView;
use crate::controller;
use crate::service::feed_subscription_service::Subscription;
use crate::view_core;

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
        let mut view = FeedListView::new(&ctx, subscriptions, pagination);

        view.render().await?;

        while let Some((action, _)) = view.listen_once().await? {
            if matches!(action, FeedListAction::Save) {
                for url in &view.marked_unsub {
                    service.unsubscribe(url, &subscriber).await?;
                }
                let total_items = service.get_subscription_count(&subscriber).await?;
                let pagination = PaginationView::new(&ctx, total_items, SUBSCRIPTIONS_PER_PAGE);
                view.pagination = pagination
            }
            let subscriptions = service
                .list_paginated_subscriptions(
                    &subscriber,
                    view.pagination.current_page(),
                    SUBSCRIPTIONS_PER_PAGE,
                )
                .await?;

            view.set_subscriptions(subscriptions);

            view.render().await?;
        }

        Ok(NavigationResult::Exit)
    }
}

pub enum FeedListState {
    View,
    Edit,
}

view_core! {
    timeout = Duration::from_secs(120),
    pub struct FeedListView<'a, FeedListAction> {
        subscriptions: Vec<Subscription>,
        pub pagination: PaginationView<'a>,
        pub state: FeedListState,
        pub marked_unsub: HashSet<String>
    }
}

impl<'a> FeedListView<'a> {
    pub fn new(
        ctx: &'a Context<'a>,
        subscriptions: Vec<Subscription>,
        pagination: PaginationView<'a>,
    ) -> Self {
        Self {
            subscriptions,
            pagination,
            core: Self::create_core(ctx),
            state: FeedListState::View,
            marked_unsub: HashSet::new(),
        }
    }

    /// Updates the subscriptions list.
    pub fn set_subscriptions(&mut self, subscriptions: Vec<Subscription>) -> &mut Self {
        self.subscriptions = subscriptions;
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

        let accessory = match self.state {
            FeedListState::View => CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                CreateUnfurledMediaItem::new(sub.feed.cover_url),
            )),
            FeedListState::Edit => {
                let source_url = sub.feed.source_url;
                let button = if self.marked_unsub.contains(&source_url) {
                    self.register(UndoUnsub { source_url })
                        .as_button()
                        .style(ButtonStyle::Secondary)
                } else {
                    self.register(Unsubscribe { source_url })
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
        let action = match self.state {
            FeedListState::Edit => FeedListAction::View,
            FeedListState::View => FeedListAction::Edit,
        };

        let state_button = self
            .register(action)
            .as_button()
            .style(ButtonStyle::Primary);

        let mut save_button = self
            .register(FeedListAction::Save)
            .as_button()
            .style(ButtonStyle::Success);

        if self.marked_unsub.is_empty() {
            save_button = save_button.disabled(true)
        }

        let buttons = vec![state_button, save_button];

        CreateComponent::ActionRow(CreateActionRow::Buttons(buttons.into()))
    }
}

impl<'a> ResponseView<'a> for FeedListView<'a> {
    fn create_response<'b>(&mut self) -> ResponseKind<'b> {
        if self.subscriptions.is_empty() {
            return Self::create_empty().into();
        }

        let sections: Vec<CreateContainerComponent<'b>> = self
            .subscriptions
            .clone()
            .into_iter()
            .map(|sub| self.create_subscription_section(sub))
            .collect();

        let container = CreateComponent::Container(CreateContainer::new(sections));
        let mut components = vec![container];

        self.pagination.attach_if_multipage(&mut components);
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
impl<'a> InteractiveView<'a, FeedListAction> for FeedListView<'a> {
    async fn handle(
        &mut self,
        action: &FeedListAction,
        interaction: &ComponentInteraction,
    ) -> Result<Option<FeedListAction>, Error> {
        use FeedListAction::*;
        match action {
            Base(pagination_action) => {
                let action = self
                    .pagination
                    .handle(pagination_action, interaction)
                    .await?;
                Ok(action.map(Base))
            }
            Edit => {
                self.state = FeedListState::Edit;
                Ok(Some(action.clone()))
            }
            View => {
                self.state = FeedListState::View;
                Ok(Some(action.clone()))
            }
            Unsubscribe { source_url } => {
                self.marked_unsub.insert(source_url.clone());
                Ok(Some(action.clone()))
            }
            UndoUnsub { source_url } => {
                self.marked_unsub.remove(source_url);
                Ok(Some(action.clone()))
            }
            Exit | Save => Ok(Some(action.clone())),
        }
    }

    async fn on_timeout(&mut self) -> Result<(), Error> {
        self.pagination.disabled = true;
        self.render().await?;
        Ok(())
    }

    fn children(&mut self) -> Vec<Box<dyn ChildViewResolver<FeedListAction> + '_>> {
        vec![Self::child(&mut self.pagination, FeedListAction::Base)]
    }
}
