# Keep the gui runtime after the settings panels retire

## Context

ADR-0009 migrated the three settings panels — feed settings, voice
settings, and welcome — into plugin crates. Their host shells and update
modules were removed. The host still owns the `/settings` GUI and the
remaining host views, so the runtime has users after the migration.

This ticket retires the host copies. The remaining host runtime has six
`GuiFeature` implementations: `about`, `settings`, `voice_stats`,
`voice_leaderboard`, `register`, and `unregister`. The last two are
one-shot features; the other four use the host event loop.

The retired panels still leave one piece of live machinery behind: the
welcome preview resolver (`PreviewResolver`, ADR-0012). It fills the
attachment slot the welcome plugin declares, and the plugin
transport paths call it. It cannot die with the panel.

## Decision

Keep the gui runtime. The host panel copies, their update cores, and
their `Navigation` targets are deleted; the runtime is not.

1. **The runtime earns its keep.** Six host features still use
   `GuiFeature`; the `/settings` GUI is one of them. Retiring the runtime
   means migrating those host features first.
2. **The commands deep-link the plugins.** `/feed settings`,
   `/voice settings`, and `/welcome` no longer start a host session.
   They call `open_plugin_view` (`src/plugin/command.rs`), the shared
   invoke → validate → defer → edit → register core extracted from
   `plugin_slash_dispatch`, and open their panel plugin's view directly.
   The helper resolves declared attachment slots through
   `PreviewResolver`, so the welcome preview works on this path too.
3. **The preview resolver moves, not dies.** `PreviewResolver`,
   `generate_preview_from`, `declares_preview`, and `WELCOME_FILE` move
   to `src/plugin/preview.rs` — consumer-side, next to the transport
   paths that call them.
4. **The hub stubs are gone for good.** With all three panels migrated,
   the `settings:config:*` fallback in `crates/plugin/settings/` is
   dead: the prefix constant, the `config_target` lookup, the
   `page_swap` stub arm, and their tests are deleted. `FEATURES` now
   carries each feature's plugin target directly.
5. **The host-owned guards stay.** `TranslateLayer` and the
   `host_owned` message checks keep routing interactions for the host
   features that still create `Host` sessions.

Plugin-side views use pure update functions, render functions, and direct
service/repository calls. The sealed `GuiFeature` and `Host` loop remain the
runtime for host-owned views; plugin views do not run through that host loop.

## Consequences

- The settings flow has two runtimes: the host `/settings` GUI and
  the three panel plugins. The host owns the hub and its section handoff;
  panel views and their slash commands use the plugin protocol.
- `Navigation::SettingsFeeds`, `SettingsVoice`, and `SettingsWelcome`
  no longer exist. `SettingsSection` hands a section to a plugin, while
  `SettingsMain` and `SettingsAbout` remain host navigation.
- The runtime's retirement question returns when the last remaining
  host `GuiFeature` migrates. Until then, deleting it would break the
  remaining host views.
- `open_plugin_view` is now the single initial-render path for plugin
  views opened from a slash interaction; `plugin_slash_dispatch` and
  the three settings commands share it.

## Update (2026-09-23)

The settings hub plugin is retired (#165). The host `/settings` command
runs the `SettingsFeature` (`src/bot/gui/settings.rs`, core
`src/update/settings.rs`), whose section click exits to
`Navigation::SettingsSection`. `SettingsMain` is a runnable host frame,
not a terminal handoff, and the About feature's Back lands on the
Settings GUI.
