-- Cleanup script: Remove duplicate voice sessions created by crash recovery
--
-- Problem: When the bot crashes while users are in voice channels, crash recovery
-- closes all active sessions with the same leave_time (last heartbeat). But if
-- the bot reconnects and the user is still in voice, a new active session is created.
-- Over multiple crashes, this creates multiple closed sessions with the same leave_time
-- that all get counted in leaderboard queries.
--
-- This script removes the duplicate closed sessions, keeping only the earliest
-- join_time per user+channel+leave_time combination.
--
-- Usage:
--   psql "$DB_URL" -f scripts/cleanup_voice_sessions.sql

BEGIN;

-- Create a CTE that identifies duplicate sessions:
-- Multiple closed sessions for the same user+channel that end at the exact same time
WITH duplicates AS (
    SELECT id
    FROM voice_sessions
    WHERE id NOT IN (
        -- Keep only the earliest join_time per user+channel+leave_time
        SELECT MIN(id)
        FROM voice_sessions
        WHERE is_active = false
        GROUP BY user_id, channel_id, leave_time
        HAVING COUNT(*) > 1
        
        UNION ALL
        
        -- Also keep all sessions that are NOT duplicates
        SELECT id
        FROM voice_sessions
        WHERE is_active = true
        
        UNION ALL
        
        SELECT id
        FROM voice_sessions vs
        WHERE is_active = false
        AND NOT EXISTS (
            SELECT 1
            FROM voice_sessions other
            WHERE other.user_id = vs.user_id
            AND other.channel_id = vs.channel_id
            AND other.leave_time = vs.leave_time
            AND other.id != vs.id
        )
    )
)
DELETE FROM voice_sessions
WHERE id IN (SELECT id FROM duplicates);

COMMIT;

-- Verify cleanup
SELECT 'Remaining closed sessions' as check_type, COUNT(*) as count
FROM voice_sessions WHERE is_active = false
UNION ALL
SELECT 'Active sessions', COUNT(*)
FROM voice_sessions WHERE is_active = true;
