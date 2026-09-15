//! Shared Components V2 reply builders for one-line prose responses.

use poise::CreateReply;
use poise::serenity_prelude::CreateComponent;
use poise::serenity_prelude::CreateContainer;
use poise::serenity_prelude::CreateContainerComponent;
use poise::serenity_prelude::CreateTextDisplay;
use poise::serenity_prelude::MessageFlags;

/// The Components V2 reply for a one-line message: the prose in a text
/// display inside a container, with no legacy content field. Accepts
/// borrowed or owned prose and returns an owned reply.
pub(crate) fn text_reply(message: impl Into<String>) -> CreateReply<'static> {
    let components = vec![CreateComponent::Container(CreateContainer::new(vec![
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(message.into())),
    ]))];
    CreateReply::default()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(components)
}

#[cfg(test)]
mod tests {
    use poise::serenity_prelude::EditInteractionResponse;

    use super::*;

    #[test]
    fn text_reply_carries_the_v2_flag_and_no_legacy_content() {
        let reply = text_reply("Plugin `hello` enabled.");

        let edit = reply.to_slash_initial_response_edit(EditInteractionResponse::new());
        let body = serde_json::to_value(&edit).expect("edit body serializes");

        assert_eq!(
            body["flags"],
            serde_json::json!(MessageFlags::IS_COMPONENTS_V2)
        );
        assert!(body.get("content").is_none());
        assert_eq!(body["components"][0]["type"], 17);
        assert_eq!(body["components"][0]["components"][0]["type"], 10);
        assert_eq!(
            body["components"][0]["components"][0]["content"],
            "Plugin `hello` enabled."
        );
    }

    #[test]
    fn text_reply_accepts_borrowed_and_owned_prose_alike() {
        let edit = text_reply("the message")
            .to_slash_initial_response_edit(EditInteractionResponse::new());
        let borrowed = serde_json::to_value(&edit).expect("edit body serializes");
        let edit = text_reply(String::from("the message"))
            .to_slash_initial_response_edit(EditInteractionResponse::new());
        let owned = serde_json::to_value(&edit).expect("edit body serializes");

        assert_eq!(borrowed, owned);
    }
}
