# Commit & Changelog Conventions

The changelog is generated from your commits automatically. Generation uses
the stock `conventional-changelog-conventionalcommits` preset. The only
custom behavior in `.config.cjs` is the version bump logic.

## Commit message format

Write commits in [Conventional Commits](https://www.conventionalcommits.org/)
format:

```
<type>(<scope>): <subject>
```

- `type` — the type of change (see below)
- `scope` — optional; the part of the codebase you changed
- `subject` — a short description of the change

The type decides the changelog section. The scope shows in the entry. The
subject becomes the entry text.

Example:

```
feat(api): add user search endpoint
fix(parser): handle empty input
```

## Which commits appear

These types create changelog entries:

| Type | Section heading | Meaning |
|------|-----------------|---------|
| `feat` | Features | A new user-facing feature |
| `fix` | Bug Fixes | A bug fix |
| `perf` | Performance Improvements | A performance improvement |
| `revert` | Reverts | A reverted change |

These types do NOT create entries:

```
docs, style, chore, refactor, test, build, ci
```

The exclusion is intentional. These commits appear often, and including
them hides the user-facing entries. They still count toward the version
bump (see below).

## Bump control

The bump type comes from the commit subject. It is independent of changelog
visibility:

| Subject | Bump |
|---------|------|
| `chore!(major): ...` | major |
| `chore!(minor): ...` | minor |
| anything else | patch |

Example: `chore!(major): drop the legacy config format` bumps the version
to the next major but creates no entry.

A visible commit can also declare a breaking change. Add `!` after the type,
or add a `BREAKING CHANGE:` footer to the body. The changelog then shows the
entry under a "Breaking Changes" section.

Workspace members (crates) are bumped independently. The CI detects changed
members by file path under `crates/<member>/`, not by commit scope. Commit
scope is a human-readable convention. It has no effect on the bump logic.

## Per-repo changelog mode

Each project chooses how its CHANGELOG.md is maintained. Create a
`.github/changelog-mode` file in the project repo. The file holds one word:
`auto` or `manual`. A missing file means `auto`.

**auto** (default) — the release workflow regenerates CHANGELOG.md from the
commits and commits it to the release branch.

**manual** — the release workflow does NOT generate CHANGELOG.md. On
release, the CI renames the topmost `## [Unreleased]` section to
`## <version> (<date>)` and commits the change. The GitHub release body and
the Discord embed read the renamed section. Version bumping and tag creation
still happen automatically. On a pull request, the preview bot shows the
materialized release body (the `[Unreleased]` section renamed to
`## <version> (<date>)`), matching what the release will post.

If the topmost section is not `## [Unreleased]` (or the file has none), the
tooling inserts one before materializing it. The inserted section is then
renamed in the same release, so the release body is just the version heading;
it never shows a previous release's notes. Add and populate a
`## [Unreleased]` section during development so the preview and release body
carry the real entries.

The file is project-local. `sync.sh` never ships it, and never pulls it.

## PR-level overrides (auto mode only)

On every pull request, the preview bot posts the generated changelog with
the next version and the crate bump table. A maintainer can override the
auto-detection. Add the sections in the PR description or in a comment on
the PR. The bot uses the latest maintainer comment that contains the
sections.

### `## Bump` — manual version bumps

```
## Bump
natmap: minor
auto-discover: patch
```

Each line sets one member's bump type. The format is
`<name>: <major|minor|patch>`. The section overrides the automatic
per-member detection in the preview.

### `## Override Changelog` — full changelog replacement

```
## Override Changelog
### Breaking Changes
- Dropped support for the legacy config format

### Features
- Added a multi-threaded file watcher
```

This text replaces the generated changelog in the preview comment. It does
not affect the release. The release still generates its own changelog.

Both sections are optional and can appear in the same comment. If neither
is present, the preview falls back to the auto-generated output.

The overrides apply only in `auto` mode. In `manual` mode the preview shows
the materialized release body (the `[Unreleased]` section renamed to
`## <version> (<date>)`). It ignores both overrides.

## Generated vs manual

| Aspect | auto | manual |
|--------|------|--------|
| CHANGELOG.md | generated and committed by the release | hand-written; `[Unreleased]` header renamed to version + date by the release |
| GitHub release body | generated changelog | renamed `[Unreleased]` section (version + date) |
| Discord embed | generated changelog | renamed `[Unreleased]` section (version + date) |
| PR preview | changelog + version + bump table | materialized release body + version |
| Version bump and tag | automatic | automatic |
