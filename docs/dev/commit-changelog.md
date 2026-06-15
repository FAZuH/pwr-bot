# Commit & Changelog Conventions

## `[pub]` marker

Only commits tagged with `[pub]` (or `[public]`) appear in the changelog. The
marker can be anywhere in the commit subject or body. It is stripped from the
final changelog entry automatically.

```
feat: Add batch processing [pub]
docs: Fix typo in readme [public]
```

Commits without `[pub]` are **silently excluded** from the changelog. Use this
for things like refactors, CI changes, dev tooling, or any internal-only work.

### When to use `[pub]` — two audiences

The definition of "user-facing" depends on your project's audience:

- **Developers (library / crate)** — your users are other developers. Mark
  commits with `[pub]` when they affect the public API, add new features,
  improve performance, or fix bugs. Skip internal refactors, test additions,
  CI changes, or dev documentation.

- **App consumers (binary)** — your users are end users of the application.
  Mark commits with `[pub]` for UI changes, new features, performance
  improvements, bug fixes, or user-facing documentation. Skip refactors,
  developer tooling, internal docs, CI, or test-only changes.

| Audience    | `[pub]` (include)                           | No marker (exclude)               |
|-------------|---------------------------------------------|-----------------------------------|
| Developer   | API changes, new features, perf, bug fixes  | Refactors, CI, dev docs, tests    |
| App user    | UI changes, new features, perf, bug fixes   | Refactors, CI, dev tooling, tests |

## Commit message format

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <subject>
```

- **type** — `feat`, `fix`, `perf`, `docs`, `refactor`, `test`, `ci`, `chore`,
  `style`, `build`, `revert`
- **scope** — optional; reflects the area of the codebase being changed
- **subject** — capitalize the first letter; it appears verbatim in the
  changelog (unless we override the changelog).

```
feat(config): Add support for YAML config files [pub]
fix(parser): Handle null values gracefully [pub]
```

### Bump control

Bump type is controlled by the commit subject, independent of `[pub]`:

| Subject                  | Bump    |
|--------------------------|---------|
| `chore!(major): ...`     | major   |
| `chore!(minor): ...`     | minor   |
| everything else          | patch   |

Workspace members are bumped independently based on **file path changes**
under `crates/<member>/`, not commit scope. The CI runs `git diff` since the
last tag to detect which member directories have changes. Commit scope is a
human-readable convention and has no effect on bump logic.

### Commit body overrides

Additional fields in the commit body modify how entries appear in the
changelog:

```
feat(api): add user search endpoint [pub]

scope: Users
changelog: New `/api/users/search` endpoint with pagination
```

- **`scope:`** — overrides the changelog section/group for this entry.
  Defaults to the commit type label (e.g., "New Features", "Bug Fixes").
- **`changelog:`** — replaces the commit subject in the changelog with a
  custom message. Useful when the subject is too terse or technical.

## PR-level overrides

Add sections to any **maintainer-authored PR comment** (not the PR body) to
override auto-detection. The bot picks up the latest maintainer comment
containing overrides.

### `## Bump` — manual version bumps

```
## Bump
natmap: minor
auto-discover: patch
```

Each line: `<scope>: <major|minor|patch>`. Overrides the automatic
scope-based detection for workspace crate bumps.

### `## Override Changelog` — full changelog replacement

```
## Override Changelog
### Breaking Changes
- Dropped support for legacy config format (v1)

### New Features
- Added multi-threaded file watcher
- New `--watch` flag for live reload
```

Replaces the entire auto-generated changelog. Also skips the TriPSs
generation entirely.

Both sections are optional and can be used together in the same comment.
`## Bump` controls the version bump table; `## Override Changelog` replaces
the generated changelog text. Each overrides its respective auto-detection.
If neither is present, the system falls back to auto-detection.
