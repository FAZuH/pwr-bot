# Test TEA feature cycles offline through rendered labels

The retired `gui_test` slash command and its test framework exercised
host views through live Discord round trips: a scripted step ran against
the real gateway, so it needed the bot running and left no test-suite
evidence — the deleted `gui_test` and `test_framework` files contained
no `#[test]`s. Only the boot-load data ever needed the live context;
the update cycle itself is pure (ADR-0005).

Feature cycles now run offline as pure update tests. Each test drives
one seam of the `GuiFeature` contract with constructed boot data — no
Discord, no DB: render the view, pick an action, translate it to a
`Msg`, apply it through `update`, and assert the effects, the model,
and the re-rendered snapshot. Boot-load data enters as `Config`
data-in at construction, the same shape the Host receives.

The selectors are visible labels, never custom ids.
`find_by_rendered_label` in the `cycle` module (`src/bot/gui/mod.rs`)
renders the view, locates the button whose visible text equals the
label, and resolves its custom id through the registry;
`debug_assert_unique_labels` fails a debug build when two actions share
a label, because label lookup would otherwise depend on hash-map order.
A test breaks when a visible label changes. We accept that: the label
is what the user sees, and a renamed button must fail the test that
claims the user can click it. Custom ids carry timestamps, so they are
useless as offline selectors; snapshots normalize them to `id:Type`
sentinels.

The trade-off is live coverage. The suite no longer proves that a click
round-trips through Discord's real collectors, and the `pwr-viewgen`
preview loop stays a human tool — no HTML assertions stand in for the
missing round trips. What we get in return: deterministic tests that
run in `cargo test` everywhere, with no gateway, no database, and no
Discord timing to wait on.
