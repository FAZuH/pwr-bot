# Migrate the host feature panels to plugin views

The settings hub renders three `settings:config:<feature>` buttons that are
documented stubs. A click re-renders the current page until the per-feature
panels exist as plugins (`CUSTOM_ID_CONFIG_PREFIX` in
`crates/plugin/settings/src/main.rs`).

The panels behind the buttons — feed settings, voice settings, and welcome —
are the last host-owned views in the settings flow. Each is a TEA view the
host gui runtime drives (ADR-0005), reached through its slash command
(`src/bot/command/feed/settings.rs`, `src/bot/command/voice/settings.rs`,
`src/bot/command/welcome/mod.rs`) or through a stub button. The hub's nav row
already opens plugin views: the `settings:open:<plugin>` pattern calls
`host.open_view` (`open_view_call` in `src/plugin/host.rs`).

Two options existed for closing the gap. Option A kept the panels
host-owned. It added a takeover navigation op: a plugin click made the host
morph the plugin's message into one of its own panel views. The hub handoff
already morphs in the other direction (`adopt_message_into_hub` in
`src/bot/command/session_exit.rs`), so the takeover op had a precedent to
copy. Option B migrated the panels themselves into plugin crates.

We chose option B, approved by the user on 2026-09-05. All views in the
settings flow become plugins, so view ownership never splits between two
runtimes. A takeover op keeps the host runtime alive as a permanent
second view owner. It also adds new host machinery to serve views that
will move anyway. The stub comment's original plan was migration in the
first place.

The decision fixes the migration shape:

1. **One plugin crate per panel.** `feed-settings`, `voice-settings`, and
   `welcome` become separate plugins under `crates/plugin/`.
2. **Hub discovery is automatic.** The hub's nav row already builds itself
   from the running plugins through `host.list_plugins`, so each panel
   plugin appears without hub changes.
3. **Independent shipping.** Each panel plugin ships and hot-plugs on its
   own through the plugin catalog (`src/plugin/install.rs`).
4. **The stubs rewire.** As each panel lands, its `settings:config:*`
   button becomes `settings:open:` navigation, and the stub disappears
   with the last panel.

The host gui runtime loses the three panels one migration at a time
(`GuiFeature` in `src/bot/gui/feature.rs`, the `Host` loop in
`src/bot/gui/rt.rs`). Other features keep the runtime alive after they
leave: about, feed list, feed batch, voice stats, and the voice
leaderboard drive the `Host` loop, and the one-shot register and
unregister run on the same `GuiFeature` shell. What happens to the
runtime once no features remain is an open question. Whether to retire
`GuiFeature` and `rt.rs`, and when, is not settled by this decision.

The panels read and write per-service data, so service access must cross
the process boundary. ADR-0010 records the RPC policy for that seam.

Welcome opens a modal today through its `open_modal` hook
(`src/bot/gui/welcome.rs`), the only such impl in the host gui, and the
plugin protocol has no modal op. The capability must land before welcome
migrates. ADR-0011 records it.

ADR-0005 records the TEA runtime these panels leave behind.
