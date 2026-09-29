// @generated automatically by Diesel CLI.

diesel::table! {
    welcome_settings (guild_id) {
        guild_id -> Int8,
        enabled -> Bool,
        channel_id -> Nullable<Text>,
        primary_color -> Nullable<Text>,
        template_id -> Nullable<Text>,
        messages -> Nullable<Jsonb>,
    }
}
