pub struct VoiceCog;
// use std::time::Duration;
//
// use chrono::Utc;
// use poise::CreateReply;
// use serenity::all::ComponentInteractionCollector;
// use serenity::all::ComponentInteractionDataKind;
// use serenity::all::CreateActionRow;
// use serenity::all::CreateComponent;
// use serenity::all::CreateContainer;
// use serenity::all::CreateContainerComponent;
// use serenity::all::CreateInteractionResponse;
// use serenity::all::CreateInteractionResponseMessage;
// use serenity::all::CreateSelectMenu;
// use serenity::all::CreateSelectMenuKind;
// use serenity::all::CreateSelectMenuOption;
// use serenity::all::CreateTextDisplay;
// use serenity::all::MessageFlags;
//
// use crate::bot::commands::Context;
// use crate::bot::commands::Error;
// use crate::bot::error::BotError;
// use crate::bot::views::PageNavigationView;
// use crate::bot::views::Pagination;
// use crate::database::model::ServerSettings;
// use crate::service::voice_tracking_service::VoiceTotalMemberData;
//
// pub struct VoiceCog;
//
// impl VoiceCog {
//     #[poise::command(
//         slash_command,
//         subcommands("Self::settings", "Self::leaderboard", "Self::history")
//     )]
//     pub async fn vc(_ctx: Context<'_>) -> Result<(), Error> {
//         Ok(())
//     }
//
//     #[poise::command(
//         slash_command,
//         default_member_permissions = "ADMINISTRATOR | MANAGE_GUILD"
//     )]
//     pub async fn settings(ctx: Context<'_>) -> Result<(), Error> {
//         use serenity::futures::StreamExt;
//         ctx.defer().await?;
//         let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
//
//         let mut settings = ctx
//             .data()
//             .service
//             .voice_tracking
//             .get_server_settings(guild_id)
//             .await?;
//
//         let msg_handle = ctx.send(SettingsVcView::create(&settings)).await?;
//
//         let msg = msg_handle.message().await?.into_owned();
//         let author_id = ctx.author().id;
//
//         let mut collector = ComponentInteractionCollector::new(ctx.serenity_context())
//             .message_id(msg.id)
//             .author_id(author_id)
//             .timeout(Duration::from_secs(120))
//             .stream();
//
//         while let Some(interaction) = collector.next().await {
//             let mut should_update = true;
//
//             match &interaction.data.kind {
//                 ComponentInteractionDataKind::StringSelect { values }
//                     if interaction.data.custom_id == "voice_settings_enabled" =>
//                 {
//                     if let Some(value) = values.first() {
//                         settings.voice_tracking_enabled = Some(value == "true");
//                     }
//                 }
//                 _ => {
//                     should_update = false;
//                 }
//             }
//
//             if should_update {
//                 ctx.data()
//                     .service
//                     .voice_tracking
//                     .update_server_settings(guild_id, settings.clone())
//                     .await?;
//             }
//
//             interaction
//                 .create_response(
//                     ctx.http(),
//                     CreateInteractionResponse::UpdateMessage(
//                         CreateInteractionResponseMessage::new()
//                             .components(SettingsVcView::create_settings_components(&settings)),
//                     ),
//                 )
//                 .await?;
//         }
//
//         Ok(())
//     }
//
//     #[poise::command(slash_command)]
//     pub async fn leaderboard(ctx: Context<'_>) -> Result<(), Error> {
//         ctx.defer().await?;
//         let guild_id = ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?.get();
//
//         let leaderboard = ctx
//             .data()
//             .service
//             .voice_tracking
//             .get_leaderboard(guild_id, 10)
//             .await?;
//
//         if leaderboard.is_empty() {
//             ctx.say("No voice activity recorded yet.").await?;
//             return Ok(());
//         }
//
//         let view = VoiceLeaderboardView::new(&ctx).await?;
//
//         Ok(())
//     }
//
//     #[poise::command(slash_command)]
//     pub async fn history(ctx: Context<'_>) -> Result<(), Error> {
//         ctx.say("🚧 This command is coming soon!").await?;
//         Ok(())
//     }
// }
//
// struct VoiceLeaderboardView<'a> {
//     ctx: &'a Context<'a>,
//     navigation: PageNavigationView<'a>,
// }
//
// impl<'a> VoiceLeaderboardView<'a> {
//     async fn new(ctx: &'a Context<'a>) -> Result<Self, Error> {
//         let until = Utc::now();
//         let from = until - Duration::from_hours(24);
//         let total_items = ctx
//             .data()
//             .service
//             .voice_tracking
//             .get_voice_user_count(
//                 ctx.guild_id().ok_or(BotError::GuildOnlyCommand)?,
//                 &from,
//                 &until,
//             )
//             .await?;
//
//         let pages = total_items.div_ceil(10);
//         let navigation = PageNavigationView::new(ctx, Pagination::new(pages, 10, 1));
//         Ok(Self { ctx, navigation })
//     }
//
//     fn create_page(&self) -> Vec<CreateComponent<'a>> {
//         todo!()
//     }
//
//     fn create_empty() -> Vec<CreateComponent<'a>> {
//         vec![CreateComponent::Container(CreateContainer::new(vec![
//             CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
//                 "No voice data found in this server.",
//             )),
//         ]))]
//     }
//
//     fn create<'b>(
//         partner_data_total: VoiceTotalMemberData,
//         author_data_total: VoiceTotalMemberData,
//         member_data_total: Vec<VoiceTotalMemberData>,
//         pagination: &'a PageNavigationView,
//     ) -> Vec<CreateComponent<'b>> {
//         todo!();
//         let container = CreateComponent::Container(CreateContainer::new(vec![]));
//
//         pagination.append_buttons_if_multipage(vec![container])
//     }
// }
//
// struct VoiceHistoryView;
//
// impl VoiceHistoryView {}
//
// struct SettingsVcView;
//
// impl SettingsVcView {
//     pub fn create(settings: &ServerSettings) -> CreateReply<'_> {
//         CreateReply::new()
//             .flags(MessageFlags::IS_COMPONENTS_V2)
//             .components(Self::create_settings_components(settings))
//     }
//
//     pub fn create_settings_components(settings: &ServerSettings) -> Vec<CreateComponent<'_>> {
//         let is_enabled = settings.voice_tracking_enabled.unwrap_or(true);
//
//         // Status section
//         let status_text = format!(
//             "## Voice Tracking Settings\n\n> 🛈  {}",
//             if is_enabled {
//                 "Voice tracking is **active**."
//             } else {
//                 "Voice tracking is **paused**."
//             }
//         );
//         let enabled_select = CreateSelectMenu::new(
//             "voice_settings_enabled",
//             CreateSelectMenuKind::String {
//                 options: vec![
//                     CreateSelectMenuOption::new("🟢 Enabled", "true").default_selection(is_enabled),
//                     CreateSelectMenuOption::new("🔴 Disabled", "false")
//                         .default_selection(!is_enabled),
//                 ]
//                 .into(),
//             },
//         )
//         .placeholder("Toggle voice tracking");
//
//         let container = CreateComponent::Container(CreateContainer::new(vec![
//             CreateContainerComponent::TextDisplay(CreateTextDisplay::new(status_text)),
//             CreateContainerComponent::ActionRow(CreateActionRow::SelectMenu(enabled_select)),
//         ]));
//
//         vec![container]
//     }
// }

use poise::Command;

use crate::bot::Data;
use crate::bot::commands::Cog;
use crate::bot::commands::Error;

impl Cog for VoiceCog {
    fn commands(&self) -> Vec<Command<Data, Error>> {
        vec![]
    }
}
