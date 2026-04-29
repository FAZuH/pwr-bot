-- Remove is_active column by dropping and recreating table
DROP TABLE IF EXISTS voice_sessions;

CREATE TABLE IF NOT EXISTS voice_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    guild_id INTEGER NOT NULL,
    channel_id INTEGER NOT NULL,
    join_time TIMESTAMP NOT NULL,
    leave_time TIMESTAMP NOT NULL,
    UNIQUE(user_id, channel_id, join_time)
);
