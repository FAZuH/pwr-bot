#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "psycopg2-binary",
# ]
# ///
"""Migrate data from SQLite to PostgreSQL for pwr-bot.

Usage:
    export DB_URL="postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot"

This script reads from data/prod.db (SQLite) and writes to the PostgreSQL
database configured by DB_URL. It preserves IDs to maintain FK
relationships and resets SERIAL sequences afterward.
"""

import json
import os
import sqlite3
from datetime import datetime, timezone
from typing import Any

import psycopg2
from psycopg2.extras import Json as PgJson

SQLITE_PATH = os.path.join(os.path.dirname(__file__), "..", "data", "prod.db")
PG_URL = os.environ.get(
    "DB_URL", "postgres://pwr_bot:pwr_bot@localhost:5432/pwr_bot"
)


def parse_sqlite_timestamp(val: str | None) -> datetime | None:
    """Parse SQLite timestamp string into timezone-aware datetime."""
    if val is None:
        return None
    try:
        dt = datetime.fromisoformat(val.replace("Z", "+00:00"))
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt
    except ValueError:
        raise ValueError(f"Unable to parse timestamp: {val!r}")


def migrate_table(
    sqlite_cur: sqlite3.Cursor,
    pg_cur: psycopg2.extensions.cursor,
    table: str,
    columns: list[str],
    converters: dict[str, Any] | None = None,
) -> int:
    """Copy all rows from SQLite table to PostgreSQL table."""
    converters = converters or {}
    sqlite_cur.execute(f"SELECT {', '.join(columns)} FROM {table}")
    rows = sqlite_cur.fetchall()
    if not rows:
        print(f"  {table}: no rows to migrate")
        return 0

    placeholders = ", ".join(["%s"] * len(columns))
    col_names = ", ".join(columns)
    pg_cur.executemany(
        f"INSERT INTO {table} ({col_names}) VALUES ({placeholders})",
        [
            tuple(
                converters.get(col, lambda x: x)(val)
                for col, val in zip(columns, row)
            )
            for row in rows
        ],
    )
    print(f"  {table}: migrated {len(rows)} rows")
    return len(rows)


def reset_sequence(
    pg_cur: psycopg2.extensions.cursor, table: str, column: str = "id"
) -> None:
    """Reset SERIAL sequence to MAX(id) + 1."""
    pg_cur.execute(
        f"SELECT setval(pg_get_serial_sequence('{table}', '{column}'),"
        f" COALESCE((SELECT MAX({column}) FROM {table}), 0) + 1, false)"
    )


def main() -> None:
    print(f"SQLite source: {SQLITE_PATH}")
    print(f"PostgreSQL target: {PG_URL}")

    sqlite_conn = sqlite3.connect(SQLITE_PATH)
    sqlite_conn.row_factory = sqlite3.Row
    sqlite_cur = sqlite_conn.cursor()

    pg_conn = psycopg2.connect(PG_URL)
    pg_cur = pg_conn.cursor()

    # Disable FK checks during migration (not needed for PG but ensures order
    # doesn't matter as much; we insert in dependency order anyway).
    pg_cur.execute("SET session_replication_role = 'replica';")

    # Truncate target tables to ensure clean state
    print("\nTruncating PostgreSQL tables...")
    pg_cur.execute(
        "TRUNCATE TABLE bot_meta, feed_items, feed_subscriptions, feeds, "
        "server_settings, subscribers, voice_sessions RESTART IDENTITY CASCADE"
    )
    pg_conn.commit()

    # 1. subscribers (no FKs)
    print("\nMigrating subscribers...")
    migrate_table(
        sqlite_cur, pg_cur, "subscribers",
        ["id", "type", "target_id"],
    )

    # 2. feeds (no FKs)
    print("\nMigrating feeds...")
    migrate_table(
        sqlite_cur, pg_cur, "feeds",
        ["id", "name", "description", "platform_id", "source_id",
         "items_id", "source_url", "cover_url", "tags"],
        converters={
            "description": lambda v: v or "",
            "cover_url": lambda v: v or "",
            "tags": lambda v: v or "",
        },
    )

    # 3. feed_items (FK: feeds)
    print("\nMigrating feed_items...")
    migrate_table(
        sqlite_cur, pg_cur, "feed_items",
        ["id", "feed_id", "description", "published"],
        converters={"published": parse_sqlite_timestamp},
    )

    # 4. feed_subscriptions (FKs: feeds, subscribers)
    print("\nMigrating feed_subscriptions...")
    migrate_table(
        sqlite_cur, pg_cur, "feed_subscriptions",
        ["id", "feed_id", "subscriber_id"],
    )

    # 5. server_settings
    print("\nMigrating server_settings...")
    migrate_table(
        sqlite_cur, pg_cur, "server_settings",
        ["guild_id", "settings"],
        converters={"settings": lambda v: PgJson(json.loads(v))},
    )

    # 6. voice_sessions
    print("\nMigrating voice_sessions...")
    migrate_table(
        sqlite_cur, pg_cur, "voice_sessions",
        ["id", "user_id", "guild_id", "channel_id",
         "join_time", "leave_time", "is_active"],
        converters={
            "join_time": parse_sqlite_timestamp,
            "leave_time": parse_sqlite_timestamp,
            "is_active": bool,
        },
    )

    # 7. bot_meta
    print("\nMigrating bot_meta...")
    migrate_table(
        sqlite_cur, pg_cur, "bot_meta",
        ["key", "value"],
    )

    pg_conn.commit()

    # Reset SERIAL sequences
    print("\nResetting sequences...")
    for table in ("feeds", "feed_items", "subscribers", "feed_subscriptions",
                  "voice_sessions"):
        reset_sequence(pg_cur, table)
    pg_conn.commit()

    pg_cur.execute("SET session_replication_role = 'origin';")
    pg_conn.commit()

    print("\nMigration complete!")

    sqlite_conn.close()
    pg_cur.close()
    pg_conn.close()


if __name__ == "__main__":
    main()
