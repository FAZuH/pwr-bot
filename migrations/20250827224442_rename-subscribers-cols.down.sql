-- Add down migration script here

PRAGMA foreign_keys = OFF;

ALTER TABLE subscribers RENAME COLUMN `type` TO subscriber_type;
ALTER TABLE subscribers RENAME COLUMN target TO subscriber_id;

PRAGMA foreign_keys = ON;
