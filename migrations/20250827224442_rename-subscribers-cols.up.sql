-- Add up migration script here

PRAGMA foreign_keys = OFF;

ALTER TABLE subscribers RENAME COLUMN subscriber_type TO `type`;
ALTER TABLE subscribers RENAME COLUMN subscriber_id TO target;

PRAGMA foreign_keys = ON;
