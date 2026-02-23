-- Add is_active column to voice_sessions table
ALTER TABLE voice_sessions ADD COLUMN is_active INTEGER NOT NULL DEFAULT 0;

-- Migrate existing active sessions (where leave_time = join_time) to is_active = 1
UPDATE voice_sessions SET is_active = 1 WHERE leave_time = join_time;
