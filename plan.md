# Plugin Decoupling Plan

**Branch:** `feat/plugin-system`
**Goal:** Fully dynamic plugin loading — drop `.so` in directory, plugin works without modifying core.

## Completed

### Session 2 (this one)
- ✅ Dead code removal (image_generator, feed_subscription, subscriber, feed.rs, feed_update)
- ✅ Navigation cleanup (removed 7 dead variants, added `SettingsPlugin`)
- ✅ Dynamic FeatureRegistry (HashMap-based model, `all_settings_panels()`, `plugin_settings` namespace)
- ✅ Dynamic .so loading (PLUGIN_DIR, init, events, tasks, FFI command handler)
- ✅ Service layer cleanup (removed `FeedSubscriptionProvider` trait + stub)
- ✅ Integration test fixes (removed 3 dead test files, fixed db_table)
- ✅ 129 tests passing, 0 errors, clean build

## Remaining

### P1 — PluginSettingsHandler (unblocks new plugin settings panels)
`SettingsPlugin { plugin_id }` exists in Navigation but `continue`s in the router. Add generic `CommandHandler` that dispatches to plugin via `dispatch_plugin_command(registry, host_ctx, plugin_id, {"action": "settings"})`.

- `src/bot/command/mod.rs`: Add `PluginSettingsHandler` struct + `CommandHandler` impl
- `src/bot/command/mod.rs`: Wire `SettingsPlugin { plugin_id }` => `Box::new(PluginSettingsHandler::new(hc, plugin_id))`

### P2 — Dynamic config flags
`Features { voice_tracking, feed_publisher, autoregister_cmds }` is hardcoded. `cb_is_feature_enabled` in `ffi_host_ctx.rs` hardcodes two names.

- `src/config.rs`: Replace `Features` typed fields with `HashMap<String, bool>`. Load all `ENABLE_*` env vars.
- `src/bot/plugin/ffi_host_ctx.rs`: Query HashMap instead of hardcoded match.

### P3 — Settings pages → plugins (deferred, large effort)
`feed/settings.rs` (275 lines) and `voice/settings.rs` (140 lines) are ViewEngine pages in core. Move to plugin crates using PluginHost callbacks (same pattern as welcome plugin).

### P4 — Diesel → raw SQL migration (decision needed)
Repo layer is ~1800 lines (Diesel ORM for feeds, voice_sessions, subscribers, server_settings).
Plugins use raw SQL via PluginHost (sqlx). Options:
- **Keep:** Architectural asymmetry, no risk.
- **Migrate:** Remove all Diesel deps. ~60 lines of raw SQL replace ~1800 lines. Delete `tests/db_table.rs`.

## Reference
- Full handoff: `/tmp/opencode/pwr-bot-plugin-decoupling-handoff.md`
- Test count: 129 passing (28 core lib + 37 db integration + 29 voice + 19 feed + 16 welcome)
- CI: `./dev.sh all` (format + lint + build + test) — clean
