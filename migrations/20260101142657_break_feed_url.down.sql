ALTER TABLE feeds RENAME COLUMN source_url TO url;

DROP INDEX idx_feeds_platform_source;

ALTER TABLE feeds DROP COLUMN platform_id;
ALTER TABLE feeds DROP COLUMN source_id;
ALTER TABLE feeds DROP COLUMN items_id;
