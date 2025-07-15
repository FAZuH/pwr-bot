use poise::serenity_prelude as serenity;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

use crate::listener::PollingListener;
// Your existing imports
use crate::{listener::Listener, Config};
use crate::action::DiscordWebhookAction;

struct Data {
    listener: Arc<RwLock<PollingListener>>,
    config: Arc<Config>
}

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

/// Subscribe to updates for a series
#[poise::command(slash_command)]
async fn subscribe(
    ctx: Context<'_>,
    #[description = "Type of series"]
    #[choices("manga", "anime")]
    series_type: String,
    #[description = "ID of the series"] series_id: String,
) -> Result<(), Error> {
    let user_id = ctx.author().id.to_string();
    let channel_id = ctx.channel_id();

    // Create a Discord webhook action for this channel
    // You'll need to create a webhook for the channel first
    let webhook_url = ctx.data().config.webhook_url.clone();
    let action = Box::new(DiscordWebhookAction::new(webhook_url).await?);

    // Subscribe the user
    {
        let mut listener = ctx.data().listener.write().await;
        listener.subscribe(&user_id, &series_id, &series_type, action).await?;
    }

    ctx.say(format!(
        "✅ Successfully subscribed to {} series `{}`",
        series_type, series_id
    ))
    .await?;

    Ok(())
}

/// Unsubscribe from updates for a series
#[poise::command(slash_command)]
async fn unsubscribe(
    ctx: Context<'_>,
    #[description = "Type of series"]
    #[choices("manga", "anime")]
    series_type: String,
    #[description = "ID of the series"] series_id: String,
) -> Result<(), Error> {
    let user_id = ctx.author().id.to_string();

    // Unsubscribe the user
    {
        let mut listener = ctx.data().listener.write().await;
        listener.unsubscribe(&user_id, &series_id).await?;
    }

    ctx.say(format!(
        "❌ Successfully unsubscribed from {} series `{}`",
        series_type, series_id
    ))
    .await?;

    Ok(())
}

/// Show help information
#[poise::command(slash_command)]
async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"]
    #[autocomplete = "poise::builtins::autocomplete_command"]
    command: Option<String>,
) -> Result<(), Error> {
    poise::builtins::help(
        ctx,
        command.as_deref(),
        poise::builtins::HelpConfiguration {
            extra_text_at_bottom: "This bot helps you subscribe to manga and anime updates!",
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

async fn get_client(config: Arc<Config>, listener: Arc<RwLock<PollingListener>>) {
    let config_clone = Arc::clone(&config);
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![subscribe(), unsubscribe(), help()],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some("!".into()),
                edit_tracker: Some(Arc::new(poise::EditTracker::for_timespan(
                    std::time::Duration::from_secs(3600),
                ))),
                ..Default::default()
            },
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    listener,
                    config: config_clone
                })
            })
        })
        .build();

    let intents = serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;
    let client = serenity::ClientBuilder::new(&config.discord_token, intents).framework(framework).await;
    client.unwrap().start().await.unwrap();
}