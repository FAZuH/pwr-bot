//! Feed list subcommand.
use std::collections::HashSet;
use std::time::Duration;

use crate::bot::commands::feed::SendInto;
use crate::bot::commands::feed::get_or_create_subscriber;
use crate::bot::commands::prelude::*;
use crate::entity::SubscriberEntity;
use crate::service::feed_subscription_service::Subscription;
use crate::service::traits::FeedSubscriptionProvider;

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
    let sent_into = sent_into.unwrap_or(SendInto::DM);
    Coordinator::new(ctx)
        .run(NavigationResult::FeedList(Some(sent_into)))
        .await?;
    Ok(())
}

controller! { pub struct FeedListController<'a> {
    send_into: SendInto
} }

#[async_trait::async_trait]
impl Controller for FeedListController<'_> {
    async fn run(&mut self, coordinator: std::sync::Arc<Coordinator<'_>>) -> Result<(), Error> {
        let ctx = *coordinator.context();
        ctx.defer().await?;

        let subscriber = get_or_create_subscriber(ctx, &self.send_into).await?;

        let service = ctx.data().service.feed_subscription.clone();

        let total_items = service.get_subscription_count(&subscriber).await?;

        let subscriptions = service
            .list_paginated_subscriptions(&subscriber, 1u32, SUBSCRIPTIONS_PER_PAGE)
            .await?;

        let pagination = PaginationView::new(total_items, SUBSCRIPTIONS_PER_PAGE);

        let view = FeedListHandler {
            subscriptions,
            pagination,
            state: FeedListState::View,
            marked_unsub: HashSet::new(),
            service: service.clone(),
            subscriber: subscriber.clone(),
        };

        let mut engine = ViewEngine::new(ctx, view, Duration::from_secs(120), coordinator.clone());

        engine.run().await?;

        Ok(())
    }
}

#[derive(PartialEq)]
pub enum FeedListState {
    View,
    Edit,
}

pub struct FeedListHandler {
    pub subscriptions: Vec<Subscription>,
    pub pagination: PaginationView,
    pub state: FeedListState,
    pub marked_unsub: HashSet<String>,
    pub service: std::sync::Arc<dyn FeedSubscriptionProvider>,
    pub subscriber: SubscriberEntity,
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
                    registry
                        .register(UndoUnsub { source_url })
                        .as_button()
                        .style(ButtonStyle::Secondary)
                } else {
                    registry
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
    fn create_toggle_button<'b>(
        &self,
        registry: &mut ActionRegistry<FeedListAction>,
    ) -> CreateComponent<'b> {
        let action = match self.state {
            FeedListState::Edit => FeedListAction::View,
            FeedListState::View => FeedListAction::Edit,
        };

        let state_button = registry
            .register(action.clone())
            .as_button()
            .style(ButtonStyle::Primary);
        let mut save_button = registry
            .register(FeedListAction::Save)
            .as_button()
            .style(ButtonStyle::Success);

        if self.marked_unsub.is_empty() {
            save_button = save_button.disabled(true)
        }

        let buttons = vec![state_button, save_button];

        CreateComponent::ActionRow(CreateActionRow::Buttons(buttons.into()))
    }

    async fn update_subs(&mut self) -> Result<(), Error> {
        let subs = self
            .service
            .list_paginated_subscriptions(
                &self.subscriber,
                self.pagination.state.current_page,
                SUBSCRIPTIONS_PER_PAGE,
            )
            .await?;
        self.subscriptions = subs;
        Ok(())
    }
}

impl ViewRender<FeedListAction> for FeedListHandler {
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
    #[label = "✎ Edit Subscriptions"]
    Edit,
    #[label = "👁 View Mode"]
    View,
    #[label = "🗑 Unsubscribe"]
    Unsubscribe { source_url: String },
    #[label = "↶ Undo"]
    UndoUnsub { source_url: String },
    Save,
    Exit,
}}

#[async_trait::async_trait]
impl ViewHandler<FeedListAction> for FeedListHandler {
    async fn handle(&mut self, ctx: ViewContext<'_, FeedListAction>) -> Result<ViewCommand, Error> {
        use FeedListAction::*;
        match ctx.action() {
            Base(inner) => {
                let cmd = self
                    .pagination
                    .handle(ctx.map(*inner, FeedListAction::Base))
                    .await?;
                self.update_subs().await?;
                return Ok(cmd);
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
            Exit => return Ok(ViewCommand::Continue),
            Save => {
                self.state = FeedListState::View;
                for sub in &self.marked_unsub {
                    self.service.unsubscribe(sub, &self.subscriber).await?;
                }
                self.marked_unsub.clear();
                self.update_subs().await?;
            }
        };

        Ok(ViewCommand::Render)
    }

    async fn on_timeout(&mut self) -> Result<ViewCommand, Error> {
        self.pagination.on_timeout().await
    }
}
