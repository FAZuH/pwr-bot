# Structured Logging Guidelines

## 1. Structured fields — no string interpolation in messages

The message is a static label. All variable data goes in fields.

```rust
// WRONG
info!("Plugin command '{cmd_name}' failed: {e:#}");

// CORRECT
info!(
    command.name = %cmd_name,
    error = %e,
    "plugin command failed",
);
```

Field sigils:
- `%value` — Display (strings, IDs, Discord snowflakes)
- `?value` — Debug (structs/enums you don't control, full error chains)
- bare value — primitives (u64, bool, Duration, usize)

## 2. Standard field names

| Field | Value | Sigil | Example |
|-------|-------|-------|---------|
| `guild.id` | Discord guild ID | `%guild_id` |
| `user.id` | Discord user ID | `%user_id` |
| `channel.id` | Discord channel ID | `%channel_id` |
| `command.name` | Full command string | `%cmd_name` |
| `command.args` | Command arguments JSON | `?args` |
| `plugin.name` | Plugin identifier | `%plugin` |
| `plugin.version` | Plugin version string | `%ver` |
| `event.name` | Event bus event name | `%event` |
| `ffi.command` | FFI command string | `%cmd` |
| `ffi.error` | FFI error message | `%err` |
| `task.name` | Background task name | `%task` |
| `task.interval` | Task interval in seconds | `secs` (bare u64) |
| `db.query` | SQL query or label | `%query` |
| `migration.count` | Pending migration count | `.len()` (bare) |
| `error` | Error description | `%e` |
| `error.detail` | Full error chain | `?e` |
| `duration` | Operation duration in seconds | `secs` (bare f64) |
| `http.method` | Discord API HTTP method | `%method` |

## 3. Spans — `#[instrument]` and `.instrument()`

Every async function that handles a command dispatch, event bus event, or plugin FFI call MUST be wrapped in a span.

### `#[instrument]` rules

- Always `skip_all` on functions with large or opaque arguments (`Arc<_>`, `Data`, `PluginHost`)
- Add meaningful fields explicitly via `fields(...)`
- Pre-declare fields populated later with `tracing::field::Empty`

```rust
#[instrument(
    skip_all,
    fields(
        command.name = %cmd_name,
        guild.id     = tracing::field::Empty,
    )
)]
async fn registry_command_handler(ctx: ApplicationContext<'_, Data, Error>) -> ... {
    // guild.id becomes known during execution
    Span::current().record("guild.id", &ctx.guild_id().map(|g| g.get()));
}
```

### Async span safety

Never use `span.enter()` across `.await` points. Use `.instrument(span)` on futures:

```rust
async move { /* loop body */ }
    .instrument(info_span!("poll_task", task.name = %name))
    .await;
```

### Span hierarchy

```
bot                          ← long-lived, fields: daemon="pwr-bot"
  plugin_init                  ← per-plugin, fields: plugin.name, plugin.version
  registry_command_handler     ← per-command, fields: command.name, guild.id, user.id
    ffi_invoke                 ← fields: ffi.command
    db_query                   ← fields: db.query, db.duration
  event_handler                ← per-event, fields: event.name, plugin.name
    ffi_on_event               ← fields: ffi.command
  background_task              ← per-task, fields: task.name, task.interval
```

## 4. Log level semantics

| Level | Use for |
|-------|---------|
| `error!` | Unrecoverable operation failure requiring operator attention. Plugin init failed. DB connection lost. Command handler panicked. FFI call returned error. |
| `warn!` | Self-recovered failure or degraded state. Rate limited. DB retry succeeded. Plugin returned error but fallback worked. API version mismatch. |
| `info!` | Low-frequency state transitions an operator cares about: bot startup/shutdown, plugin loaded/unloaded, command registered globally, task spawned. ONE line per user-visible operation. |
| `debug!` | Per-command dispatch, per-event handling, DB query execution, FFI call traces, health heartbeat. |
| `trace!` | Raw payloads: interaction JSON, event payload, plugin response JSON, low-level protocol data. |

The event stream from Discord is high-volume. **Every raw event MUST be logged at `debug!` or lower, never `info!`.**

## 5. Plugin crates

Plugin `.so` crates MUST NOT initialize their own tracing subscriber. The host process owns the subscriber. Plugins emit `tracing` events that are captured by the host's subscriber.

```rust
// WRONG — do not init subscriber in plugin
tracing_subscriber::fmt().init();

// CORRECT — just emit events
tracing::info!(plugin.name = "my_plugin", "task completed");
```

Plugins that need the `log` crate must use `tracing` macros directly instead:

```rust
// WRONG
log::info!("doing thing");

// CORRECT
tracing::info!("doing thing");
```

## 6. Subscriber initialization

The subscriber is initialized once in `src/logging.rs`. Workspace crates and plugins NEVER initialize tracing. Use `RUST_LOG` env var for filtering.

Default filter: `RUST_LOG=${RUST_LOG:-pwr_bot=info,pwr_bot_plugin_feed=info,pwr_bot_plugin_voice=info,pwr_bot_plugin_welcome=info}`

## 7. What NOT to log

- Discord bot tokens, user tokens, or any secrets (including in DB query params)
- Full `CommandInteraction` structs (can contain tokens) — log the relevant IDs instead
- Raw PostgreSQL connection strings with credentials — log the host and database name separately
