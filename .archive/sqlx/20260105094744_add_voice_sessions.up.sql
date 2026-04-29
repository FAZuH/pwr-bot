-- Add up migration script here
CREATE TABLE IF NOT EXISTS voice_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    guild_id INTEGER NOT NULL,
    channel_id INTEGER NOT NULL,
    join_time TIMESTAMP NOT NULL,
    leave_time TIMESTAMP NOT NULL,
    UNIQUE(user_id, channel_id, join_time)
);
