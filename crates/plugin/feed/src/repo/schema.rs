// @generated automatically by Diesel CLI.

diesel::table! {
    feed_settings (guild_id) {
        guild_id -> Int8,
        enabled -> Bool,
        channel_id -> Nullable<Text>,
        subscribe_role_id -> Nullable<Text>,
        unsubscribe_role_id -> Nullable<Text>,
    }
}

diesel::table! {
    feed_items (id) {
        id -> Int4,
        feed_id -> Int4,
        description -> Text,
        published -> Timestamptz,
    }
}

diesel::table! {
    feed_subscriptions (id) {
        id -> Int4,
        feed_id -> Int4,
        subscriber_id -> Int4,
    }
}

diesel::table! {
    feeds (id) {
        id -> Int4,
        name -> Text,
        description -> Text,
        platform_id -> Text,
        source_id -> Text,
        items_id -> Text,
        source_url -> Text,
        cover_url -> Text,
        tags -> Text,
    }
}

diesel::table! {
    subscribers (id) {
        id -> Int4,
        #[sql_name = "type"]
        type_ -> Text,
        target_id -> Text,
    }
}

diesel::joinable!(feed_items -> feeds (feed_id));
diesel::joinable!(feed_subscriptions -> feeds (feed_id));
diesel::joinable!(feed_subscriptions -> subscribers (subscriber_id));

diesel::allow_tables_to_appear_in_same_query!(
    feed_items,
    feed_settings,
    feed_subscriptions,
    feeds,
    subscribers
);
