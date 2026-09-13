# The plugin preview attachment is host-injected at transport

## Context

ADR-0011 left one open question: after the welcome panel migrates, its
live welcome-card preview image crosses the plugin wire somehow. The host
renders the card from the live settings (`WelcomeImageGenerator`,
`src/bot/command/welcome/image_generator.rs`) and the monolith panel
carried the PNG bytes in its model. #151's acceptance criteria said the
"image-transport decision (bytes vs path) is made in this ticket",
premised on "the message size caps the protocol already enforces".

## The premise was stale

The wire protocol enforces no size cap. The plugin stdout reader is a
plain `read_line` with no maximum (`src/plugin/mod.rs`), the hello-timeout
read aside, and the protocol crate declares no message limit. A JSON-line
frame can be arbitrarily large on both sides.

The size argument still stands on its own. A welcome-card PNG is
~100–500 KB; base64-in-JSON inflates it ~33%. The preview re-renders on
every mutating interaction — a toggle, a select, a saved removal — so the
bytes would ride the wire on nearly every message round trip. The
monolith never serialized the image either: its model held the bytes in
memory, and its `attachments()` hook handed them to serenity at send
time.

## Decision

The plugin never sees the image bytes. Its view envelope declares an
attachment slot by filename — the same declaration the monolith's
`attachments()` hook made — and the host fills the slot at transport:

- The plugin envelope's `data.attachments` names
  `{ "id": 0, "filename": "welcome_preview.png" }` while welcome cards are
  enabled, and `[]` while they are off. The empty list is explicit so an
  edit removes an attachment the message carried.
- The host resolves the declaration where a plugin envelope reaches
  Discord: the click path's type-7 response and its webhook-edit
  fallbacks, the modal-submission response, and `host.open_view`'s final
  edit. The resolver (`PreviewResolver`, `src/bot/gui/welcome.rs` — since
  moved to `src/plugin/preview.rs` by the #152 retirement, ADR-0013) checks
  the declaration, loads the guild's settings through the service, renders
  the card, and returns the `CreateAttachment` list to send. A slot the
  host cannot fill — no guild, no settings, a failed render — is declared
  away with an empty list, so the message never carries a dangling slot.
- The transport seam (`HostIo::edit_message`) takes the resolved
  attachment list; the raw HTTP client converts `CreateAttachment` to its
  wire form at the boundary.

The plugin keeps `RenderImage`-shaped semantics without an effect: the
host re-renders from the persisted settings on every transport, so the
preview always reflects what the panel just saved. The plugin declares
the slot; the host decides the bytes. The monolith's separation is kept —
the update logic never touches image bytes — while the transport moves
wholly host-side.

## Alternatives considered

- **Bytes in the RPC response** (base64 in `host.welcome.get_settings`
  or a dedicated `host.welcome.render_image` op): makes the plugin a
  base64 relay and puts ~130–660 KB on the wire on every load and every
  persist-and-rerender, to hand bytes back to the host that produced
  them. It also drags the attachment decision back into the plugin, which
  then must know Discord's multipart shape — the thing the host seam
  exists to own.
- **Shared temp-file path**: avoids the pipe but adds lifecycle the
  process boundary cannot share — who unlinks, when, on whose failure.
  It leaks a filesystem concern into the protocol for no gain over the
  declaration.

## Consequences

- The protocol surface grows by the settings RPC pair only; no render op,
  no bytes op. The cap count lands at 18.
- A plugin that declares a filename the host does not know gets nothing:
  the resolver only fills `welcome_preview.png` today. The mechanism is
  host-side and additive — a new preview source extends the resolver, not
  the wire.
- Snapshot tests normalize the declared attachment list alongside the
  custom-id timestamps, since the declaration is stable JSON while the
  bytes themselves never reach the plugin.
