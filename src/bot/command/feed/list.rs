//! Feed list subcommand.
use std::time::Duration;

use crate::bot::command::feed::SendInto;
use crate::bot::command::feed::get_or_create_subscriber;
use crate::bot::command::prelude::*;
use crate::entity::SubscriberEntity;
use crate::service::feed_subscription::Subscription;
use crate::service::traits::FeedSubscriptionProvider;
use crate::update::Update;
use crate::update::feed_list::FeedListCmd;
use crate::update::feed_list::FeedListModel;
use crate::update::feed_list::FeedListMsg;
use crate::update::feed_list::FeedListUpdate;
use crate::update::feed_list::FeedListViewState;

/// Number of items per page for subscriptions list.
pub(crate) const SUBSCRIPTIONS_PER_PAGE: u32 = 10;

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

        let subscriptions = service
            .list_paginated_subscriptions(&subscriber, 1u32, SUBSCRIPTIONS_PER_PAGE)
            .await?;

        let view = FeedListHandler {
            subscriptions,
            model: FeedListModel::new(SUBSCRIPTIONS_PER_PAGE),
            service: service.clone(),
            subscriber: subscriber.clone(),
        };

        let mut engine = ViewEngine::new(ctx, view, Duration::from_secs(120), coordinator.clone());

        engine.run().await?;

        Ok(())
    }
}

pub struct FeedListHandler {
    pub subscriptions: Vec<Subscription>,
    pub model: FeedListModel,
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

        let accessory = match self.model.state {
            FeedListViewState::View => CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                CreateUnfurledMediaItem::new(sub.feed.cover_url),
            )),
            FeedListViewState::Edit => {
                let source_url = sub.feed.source_url;
                let button = if self.model.marked_unsub.contains(&source_url) {
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
        let action = match self.model.state {
            FeedListViewState::Edit => FeedListAction::View,
            FeedListViewState::View => FeedListAction::Edit,
        };

        let state_button = registry
            .register(action.clone())
            .as_button()
            .style(ButtonStyle::Primary);
        let mut save_button = registry
            .register(FeedListAction::Save)
            .as_button()
            .style(ButtonStyle::Success);

        if self.model.marked_unsub.is_empty() {
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
                self.model.current_page,
                self.model.per_page,
            )
            .await?;
        self.subscriptions = subs;
        Ok(())
    }
}

impl ViewRender for FeedListHandler {
    type Action = FeedListAction;
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

        let mut pagination =
            PaginationView::new(self.subscriptions.len() as u32, self.model.per_page);
        pagination.state.current_page = self.model.current_page;
        pagination.disabled = self.model.pagination_disabled;
        pagination.attach_if_multipage(registry, &mut components, FeedListAction::Base);
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
impl ViewHandler for FeedListHandler {
    type Action = FeedListAction;
    async fn handle(&mut self, ctx: ViewContext<'_, FeedListAction>) -> Result<ViewCommand, Error> {
        use FeedListAction::*;
        match ctx.action() {
            Base(inner) => {
                FeedListUpdate::update(FeedListMsg::Pagination(*inner), &mut self.model);
                self.update_subs().await?;
                return Ok(ViewCommand::Render);
            }
            Edit => {
                FeedListUpdate::update(FeedListMsg::Edit, &mut self.model);
            }
            View => {
                FeedListUpdate::update(FeedListMsg::View, &mut self.model);
            }
            Unsubscribe { source_url } => {
                FeedListUpdate::update(
                    FeedListMsg::ToggleUnsub {
                        source_url: source_url.clone(),
                    },
                    &mut self.model,
                );
            }
            UndoUnsub { source_url } => {
                FeedListUpdate::update(
                    FeedListMsg::ToggleUnsub {
                        source_url: source_url.clone(),
                    },
                    &mut self.model,
                );
            }
            Exit => return Ok(ViewCommand::Continue),
            Save => {
                let cmd = FeedListUpdate::update(FeedListMsg::Save, &mut self.model);
                match cmd {
                    FeedListCmd::SaveUnsubscribes(urls) => {
                        for sub in urls {
                            self.service.unsubscribe(&sub, &self.subscriber).await?;
                        }
                        self.update_subs().await?;
                    }
                    FeedListCmd::RefetchSubscriptions => {
                        self.update_subs().await?;
                    }
                    FeedListCmd::None => {}
                }
            }
        };

        Ok(ViewCommand::Render)
    }

    async fn on_timeout(&mut self) -> Result<ViewCommand, Error> {
        self.model.pagination_disabled = true;
        if self.subscriptions.is_empty() {
            Ok(ViewCommand::Exit)
        } else {
            Ok(ViewCommand::RenderOnce)
        }
    }
}
