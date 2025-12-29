CREATE TABLE IF NOT EXISTS server_settings (
    guild_id INTEGER PRIMARY KEY,
    settings TEXT NOT NULL
);

-- 1. Insert settings (using the first one found per guild)
INSERT OR IGNORE INTO server_settings (guild_id, settings)
SELECT 
    substr(target_id, 1, instr(target_id, ':') - 1),
    '{"channel_id": "' || substr(target_id, instr(target_id, ':') + 1) || '"}'
FROM subscribers 
WHERE type = 'guild' AND instr(target_id, ':') > 0
GROUP BY substr(target_id, 1, instr(target_id, ':') - 1);

-- 2. Temporary table to map old subscriber_id to new subscriber_id (which is the one we keep)
CREATE TEMP TABLE subscriber_map AS
SELECT 
    s1.id as old_id,
    (
        SELECT s2.id 
        FROM subscribers s2 
        WHERE s2.type = 'guild' 
          AND substr(s2.target_id, 1, instr(s2.target_id, ':') - 1) = substr(s1.target_id, 1, instr(s1.target_id, ':') - 1)
        ORDER BY s2.id ASC
        LIMIT 1
    ) as new_id
FROM subscribers s1
WHERE s1.type = 'guild' AND instr(s1.target_id, ':') > 0;

-- 3. Update feed_subscriptions to point to new_id
-- Delete subscriptions for old_id if new_id already has that subscription.
DELETE FROM feed_subscriptions
WHERE subscriber_id IN (SELECT old_id FROM subscriber_map WHERE old_id != new_id)
  AND feed_id IN (
    SELECT fs2.feed_id 
    FROM feed_subscriptions fs2
    JOIN subscriber_map sm ON fs2.subscriber_id = sm.new_id
    WHERE sm.old_id != sm.new_id
    AND feed_subscriptions.subscriber_id = sm.old_id
  );

-- Now update the remaining ones
UPDATE feed_subscriptions
SET subscriber_id = (SELECT new_id FROM subscriber_map WHERE old_id = subscriber_id)
WHERE subscriber_id IN (SELECT old_id FROM subscriber_map WHERE old_id != new_id);

-- 4. Delete the old subscribers that we are discarding
DELETE FROM subscribers
WHERE id IN (SELECT old_id FROM subscriber_map WHERE old_id != new_id);

-- 5. Update the target_id of the remaining subscribers
UPDATE subscribers
SET target_id = substr(target_id, 1, instr(target_id, ':') - 1)
WHERE type = 'guild' AND instr(target_id, ':') > 0;

-- Drop temp table
DROP TABLE subscriber_map;