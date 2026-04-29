-- Store bot metadata like version and voice heartbeat timestamp
CREATE TABLE IF NOT EXISTS bot_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
