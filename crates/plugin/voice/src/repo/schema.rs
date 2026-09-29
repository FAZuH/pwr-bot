// @generated automatically by Diesel CLI.

diesel::table! {
    voice_sessions (id) {
        id -> Int4,
        user_id -> Int8,
        guild_id -> Int8,
        channel_id -> Int8,
        join_time -> Timestamptz,
        leave_time -> Timestamptz,
        is_active -> Bool,
    }
}

diesel::table! {
    voice_settings (guild_id) {
        guild_id -> Int8,
        enabled -> Bool,
    }
}

diesel::allow_tables_to_appear_in_same_query!(voice_sessions, voice_settings);
