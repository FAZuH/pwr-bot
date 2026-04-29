-- Add up migration script here
CREATE INDEX IF NOT EXISTS idx_voice_sessions_partner
ON voice_sessions (guild_id, channel_id, join_time, leave_time);