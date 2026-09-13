# Keep the gui runtime after the settings panels retire

## Context

ADR-0009 migrated the three settings panels — feed settings, voice
settings, and welcome — into plugin crates. The host copies stayed
behind: the `GuiFeature` shells (`src/bot/gui/feed_settings.rs`,
`src/bot/gui/voice_settings.rs`, `src/bot/gui/welcome.rs`), their pure
cores (`src/update/feed_settings.rs`, `src/update/voice_settings.rs`,
`src/update/welcome_settings.rs`), and the `Navigation` targets that
reached them. ADR-0009 left one question open: what happens to the host
gui runtime once no features remain?

This ticket retires the host copies. Before deleting anything, the
runtime's remaining users were audited. Seven `GuiFeature` impls are
live: `about`, `feed_batch`, `feed_list`, `voice_stats`, and
`voice_leaderboard` drive the `Host` event loop, and `register` and
`unregister` run one-shot on the same shell. None of them is in the
settings flow, and none has a migration ticket.

The retired panels still leave one piece of live machinery behind: the
welcome preview resolver (`PreviewResolver`, ADR-0012). It fills the
attachment slot the welcome-settings plugin declares, and the plugin
transport paths call it. It cannot die with the panel.

## Decision

Keep the gui runtime. The host panel copies, their update cores, and
their `Navigation` targets are deleted; the runtime is not.

1. **The runtime earns its keep.** Seven live features run on
   `GuiFeature` and the `Host` loop. Retiring the runtime means
   migrating those features first — a future program, not this ticket.
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
   `host_owned` message checks keep routing interactions: the seven
   live features still create `Host` sessions.

## Consequences

- The settings flow is plugin-only: the hub, the three panels, and
  their slash commands all speak the plugin protocol.
- `Navigation::SettingsFeeds`, `SettingsVoice`, and `SettingsWelcome`
  no longer exist. `SettingsMain` (the hub handoff) and `SettingsAbout`
  remain for the About feature.
- The runtime's retirement question returns when the last remaining
  `GuiFeature` migrates. Until then, deleting it would break seven
  working features.
- `open_plugin_view` is now the single initial-render path for plugin
  views opened from a slash interaction; `plugin_slash_dispatch` and
  the three settings commands share it.
