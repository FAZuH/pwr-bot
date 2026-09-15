# Commit scopes

Closed vocabulary. Every commit picks a scope from this table, or takes no
scope. Do not invent one on your own: when nothing fits, propose the new
scope to the user with a one-line definition and wait for explicit approval.

Format: `type(scope): summary` or `type: summary`.

## Scopes

| Scope | Covers |
|-------|--------|
| `bot` | `src/bot/**` |
| `deps` | Dependency manifests and lockfiles (`Cargo.toml`, `Cargo.lock`) |
| `db` | `src/repo/**`, `migrations/**`, and database test support |
| `feed` | `src/feed/**` |
| `minor` | `src/update/**` and small model/update logic |
| `plugin` | `src/plugin/**`, `crates/plugin/**`, and plugin protocol/component crates |
| `publisher` | `src/task/**` feed publisher tasks |
| `service` | `src/service/**` |
| `source` | repo-wide Rust source under `src/**` when no narrower scope owns it |
| `subscriber` | `src/subscriber/**` |
| `voice` | `src/bot/command/voice/**` |

No scope = repo-wide documentation, CI, configuration, scripts, or a change
that genuinely spans several components.

## Rules

- Types: `feat`, `fix`, `refactor`, `docs`, `chore`, `test`, `build`, and `ci`.
- Pick the scope by what changed, not by why.
- One logical change per commit. A change spanning two scopes becomes two
  commits; only fall back to no scope when splitting is impossible.
- New scopes require user approval first: propose the name, definition, and
  example subject, then wait. Never commit with an unapproved scope.
- Old commit subjects are never rewritten to match this list. The vocabulary
  applies from its introduction onward.
- Scopes are singular, lowercase, and no more than eight characters.

## Examples

```text
feat(plugin): add plugin view dispatch
fix(db): isolate integration databases
refactor(voice): simplify leaderboard pagination
docs: update architecture notes
```
