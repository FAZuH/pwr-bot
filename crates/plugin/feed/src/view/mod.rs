use pwr_ext::view_support::CreateComponent;
use pwr_ext::view_support::CreateMessage;
use pwr_ext::view_support::MessageFlags;
use serde_json::Value;

pub mod feed_batch;
pub mod feed_list;

pub fn message_data(components: Vec<CreateComponent<'static>>) -> Value {
    let message = CreateMessage::new()
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .components(components);
    serde_json::to_value(message).expect("feed view is serializable")
}
