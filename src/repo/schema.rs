diesel::table! {
    bot_meta (key) {
        key -> Text,
        value -> Text,
    }
}

diesel::table! {
    server_settings (guild_id) {
        guild_id -> Int8,
        settings -> Jsonb,
    }
}

diesel::allow_tables_to_appear_in_same_query!(bot_meta, server_settings,);
