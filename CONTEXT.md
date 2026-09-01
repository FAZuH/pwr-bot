# pwr-bot

A Discord bot with feed subscriptions and voice channel tracking. One host
process runs the bot and its plugin subprocesses.

## Language

**Plugin**:
A subprocess that the host spawns to add commands and views to the bot. It
speaks JSON-lines over stdio through `pwr-plugin-protocol`, with a `"t"` tag
on every message. The plugins live in `crates/plugin/`, for example `hello`
and `settings`.
_Avoid_: extension, add-on

**Host**:
The pwr-bot monolith process. It spawns plugins, routes their Discord
interactions, and answers the `host.*` operations they call.
_Avoid_: server, daemon

**ViewSpec**:
The envelope a plugin returns to show a view,
`{data: Value, ephemeral: bool, view: Value}` in
`crates/pwr-plugin-protocol/src/view.rs`. `data` is the raw Discord
message payload, and `view` is plugin-owned opaque state that the host
stores and hands back on interactions. `ephemeral` is `true` when the
reply is visible only to the invoking user.
_Avoid_: view payload

**Gate**:
The host's validate-only check of `ViewSpec.data` at every raw-send
boundary: initial slash dispatch, component and modal re-render, and
`host.open_view`. The host parses a clone of the payload through
`pwr_ext::prelude::CreateMessageDe`, discards the parsed value, and sends
the original JSON unchanged. A failure is a `WireError` with kind
`InvalidView`, and the host sends, registers, and commits nothing. The
gate applies to every view, whether `view!` or the components library
authored it. See Verbatim send, Components library, and ADR-0003.
_Avoid_: validator, schema check

**Verbatim send**:
The rule that the host never rewrites plugin JSON on the wire. The send
paths transmit `ViewSpec.data` exactly as the plugin produced it, never
`into_canonical_value()` output.
_Avoid_: canonicalization

**Preview loop**:
The `crates/preview` bin behind `./dev.sh preview`. It spawns a plugin over
the same protocol port that the host uses. It captures the view that a
command returns, and `pwr-viewgen` renders the payload to HTML or PNG under
`.scratch/preview/`.
_Avoid_: preview tool, fixture renderer

**Components library**:
The reusable typed builders in `crates/pwr-poise-components`, rebuilt on
`pwr-ext`, that plugins attach to compose views at runtime. This is
distinct from per-plugin `view!` authoring: plugins write fixed views with
the `pwr-ext` `view!` macro, as `hello` and the settings `about_view` do.
A view that needs runtime assembly composes this library instead, for
example the settings hub nav row. The `view!` grammar has no runtime
children splicing or conditionals yet.
See ADR-0004.
_Avoid_: pwr-ext
