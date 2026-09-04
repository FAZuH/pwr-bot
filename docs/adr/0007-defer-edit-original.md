# Defer the first response and edit the original in place

A plugin slash command first posted a placeholder — a real "Loading…"
content reply — and later edited the components-V2 payload into it.
Content set on the first response survives a later content-less edit, so
Discord rejected that edit with 50035, "cannot use legacy fields with
components V2", and the view never rendered. Command errors made the
shape worse: the error handler sent its message as a brand-new reply,
so a failing command left a second message behind.

A plugin command's first response is now always a bare defer, and the
plugin's payload rides the edit of the original response. No placeholder
message is sent, and no placeholder-then-followup pair exists: the
deferred response becomes the view, one message per command.
`view_presentation` in `src/plugin/command.rs` fixes the two calls of
the initial render. The plugin is invoked before any response, so the
defer can carry the view's ephemerality — Discord honors it on the
first response only — and the payload goes out verbatim through the
gate (ADR-0003). The cost the user sees is the thinking state instead
of instant content while the plugin computes the view. Re-renders
commit the returned view and edit the same message.

The error handler follows the shape: when the interaction already has
an initial response, deferred or replied, it edits the error into that
response instead of sending a new one (`ErrorHandler::send_component`
in `src/bot/error_handler.rs`), which also preserves the response's
ephemerality. It replies fresh only when no initial response exists.

The shape buys one message per command at two accepted costs. A
modal-triggering action must skip the acknowledge: opening the modal is
itself the response, so the Host consumes the interaction without an
ack or a re-render, and the submission arrives later as a message (the
`open_modal` hook in `src/bot/gui/rt.rs`). And a modal submission that
arrives after the session ends is stale: the global handler
acknowledges it, the modal task's own ack then fails with
`AlreadyResponded`, and the feature swallows both. The session that
could act on the submission no longer exists, so there is nothing to
recover.
