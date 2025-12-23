-- Feed (computing): a facility for notifying the user of a blog or other frequently updated website that new content has been added.
--
-- Each entry is uniquely identifiable by `id` and `url`
CREATE TABLE IF NOT EXISTS feeds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    tags TEXT DEFAULT NULL
);

-- 
CREATE TABLE IF NOT EXISTS feed_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feed_id INTEGER NOT NULL,
    description TEXT NOT NULL,
    published TIMESTAMP NOT NULL,
    FOREIGN KEY (feed_id) REFERENCES feeds(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
);

-- Entities subscribed to feeds
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    UNIQUE(type, target_id)
);

-- Junction table to define many-to-many relationship between `feeds` and `subscribers` table.
CREATE TABLE IF NOT EXISTS feed_subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feed_id INTEGER NOT NULL,
    subscriber_id INTEGER NOT NULL,
    UNIQUE(feed_id, subscriber_id),
    FOREIGN KEY (feed_id) REFERENCES feeds(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE,
    FOREIGN KEY (subscriber_id) REFERENCES subscribers(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE
);
