# Validate plugin views at the send boundary without rewriting them

Plugin views cross the IPC boundary as an opaque `serde_json::Value`. The
host must not let a malformed payload reach Discord, and it must not rewrite
the plugin's JSON. A validate-only gate meets both requirements.

`validate_view_data` in `src/plugin/view.rs` parses a clone of
`ViewSpec.data` through `pwr_ext::prelude::CreateMessageDe`. The gate
discards the parsed builder mirror. The send paths then transmit the
original JSON verbatim, never `into_canonical_value()` output. The gate runs
at every raw-send boundary:

1. Initial slash dispatch: `plugin_slash_dispatch` in
   `src/plugin/command.rs`
2. Component and modal re-render: `route_view_interaction` in
   `src/bot/mod.rs`
3. The `host.open_view` op in `src/plugin/host.rs`.

An invalid payload fails before any send, registration, or commit. The
initial dispatch sends no loading reply and registers no session. The
re-render is transactional: an invalid returned view leaves the prior
session view and `last_active` unchanged. `host.open_view` validates before
it posts the placeholder, so an invalid view leaks no placeholder and no
session. Failures are first-class `WireError` values with kind
`InvalidView`. The `ViewValidationError` type (thiserror) converts at the
boundary.

The gate keeps the interaction engine independent of Discord.
`interact_validated` in `src/plugin/interaction.rs` takes a closure that
returns `Result<(), WireError>`. The engine holds raw `Value` types, and the
host boundary supplies the schema check.

A plugin can rely on schema checks without the host rewriting its JSON.
When a new field becomes required, the gate makes it visible. The `tts`
and `enforce_nonce` fields are one example. The components library emits
them explicitly so its envelopes pass the gate.
