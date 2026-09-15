# Publish typed service RPCs across the plugin boundary

A plugin runs in a subprocess. It shares no memory with the host, and it
cannot call a service directly. The only channel is the host-op protocol: a
plugin sends a `Call` message with an op string, and the host answers it in
`handle_host_call` (`src/plugin/host.rs`). The v1 surface holds eleven ops
after `host.stats` landed — Discord I/O, the plugin kv store, host config,
navigation, and live stats (`crates/pwr-plugin-protocol/src/caps.rs`). No
op reaches the host's services.

The panels from ADR-0009 read and write per-service settings. Their
EffectHandlers call the service traits directly today —
`get_server_settings` and `update_server_settings` on the feed and voice
services (`src/service/traits.rs`) — through Context-free adapters
(`src/bot/gui/feed_settings.rs`, `src/bot/gui/voice_settings.rs`,
`src/bot/gui/welcome.rs`). Those calls must cross the process boundary,
and the protocol must say how.

We decided that the host publishes typed RPC endpoints that mirror its own
service methods — `host.feed.get_settings`, for example. Each endpoint has
a fixed argument schema, like `parse_send_message` has today
(`src/plugin/host.rs`). A service stays the single source of truth for its
data. A plugin stays a thin client: it calls the endpoint, gets typed data
back, and renders. The calls map close to one-to-one onto the service
traits the handlers already use.

The governing rule: ops are shaped by services, never by plugins. A plugin
is data (a catalog entry). A capability is code (host-owned API growth).
The host API grows only when a service grows, and an op names a service
and one of its methods — never a plugin.

The dependency direction stays plugins → protocol ← host. Both sides
depend on the `pwr-plugin-protocol` crate, which is the shared contract.
Neither concrete side depends on the other. This is Dependency Inversion
at the process seam.

The consequences:

- **Adding a plugin needs zero host changes.** A plugin is a catalog
  entry. It calls ops the host already serves, and no op names a plugin.
- **Adding a capability grows the op surface additively, in one place.**
  The addition touches the enum variant, the `ALL_CAPS` list, and
  `as_str` in the caps module (`crates/pwr-plugin-protocol/src/caps.rs`
  documents the procedure), plus one dispatch arm in `handle_host_call`.
  Nothing else changes.
- **The failure mode to avoid is the bespoke per-plugin op.** An op named
  after a plugin — `host.welcome.set_color`, for example — inverts the
  dependency. The host API grows because a plugin asked for it, and the
  op surface mirrors plugins instead of services.
- **The rejected alternative is panels owning their settings state in the
  plugin kv store** (`host.kv.get`, `host.kv.set`, `host.kv.delete`).
  That inverts data ownership, splits the source of truth, and forces
  service rewrites for code outside the panels that reads the same
  settings — voice tracking checks `is_enabled` per guild
  (`src/service/traits.rs`).

ADR-0009 records the migration that creates the need. ADR-0011 records
the modal capability the same seam needs.
