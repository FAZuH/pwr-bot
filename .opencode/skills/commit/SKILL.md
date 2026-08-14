---
name: commit
description: Create git commits following the project's Conventional Commits specification. Handles commit message formatting, types, scopes, and CI skip patterns.
---

# Commit Conventions

This project follows the **Conventional Commits** specification for clear and consistent commit history.

## Commit Message Format

```
<type>(<scope>): <subject>

<body>

<footer>
```

## Commit Types

| Type | Description |
|------|-------------|
| `feat` | New feature |
| `fix` | Bug fix |
| `docs` | Documentation only changes |
| `style` | Code style changes (formatting, semicolons, etc.) |
| `refactor` | Code refactoring |
| `perf` | Performance improvements |
| `test` | Adding or updating tests |
| `chore` | Build process, dependencies, etc. |
| `build` | Build system or dependency changes |
| `ci` | CI/CD changes |

## Version Bumps

Use `chore!(major)` or `chore!(minor)` in the subject to trigger a major or minor version release:

- `chore!(major): Breaking API change` → bumps major version
- `chore!(minor): New release cycle` → bumps minor version

If neither is present, the release defaults to a patch bump.

## Scopes

Use scopes to indicate which part of the codebase changed:

- `(bot)` - Discord bot commands and views
- `(db)` - Database/repository layer
- `(voice)` - Voice tracking features
- `(feed)` - Feed subscription features
- `(update)` - Pure business logic in `src/update/`

**Examples:**
- `feat(voice): Add /vc stats command`
- `fix(db): Correct SQL query for daily average calculation`
- `refactor(bot): Restructure commands module file tree`

## Guidelines

### Subject Line Rules
- Use **present tense** ("Add feature" not "Added feature")
- Use **imperative mood** ("Move cursor to..." not "Moves cursor to...")
- Keep subject under 50 characters when possible
- No period at end of subject

### Body Rules
- Separate subject from body with a blank line
- Use body to explain **what** and **why**, not **how**
- Include motivation for change and contrast with previous behavior
- Use bullet points for multiple changes (prefix with `-`)

### Footer Rules
- Reference issues: `Closes #123`, `Fixes #456`
- CI skip: `[skip ci]`, `[no ci]`, `[ci skip]`, `[skip actions]`, `[actions skip]`

## Examples

### Feature Commit
```
feat(bot): Add /vc stats command with contribution grid

Add voice activity statistics command that displays historical
data using GitHub-style contribution heatmaps.

- Support user and guild stats views
- Add time range selection
- Display total time, average, streak, and most active day
```

### Bug Fix Commit
```
fix(bot): Fix swapped unsubscribe and undo button
```

### Internal Refactor
```
refactor: Separate model & update logic

Move pure business logic from handlers into src/update/ modules
following the TEA Update pattern.
```

### Documentation Commit
```
docs: Update AGENTS.md [skip ci]

Update commit conventions and add update pattern documentation.
```

### Version Bump
```
chore!(minor): Start new release cycle
```

## CI Integration

### Skip CI Commits
Use a CI skip marker for commits that don't need CI validation:
- Documentation only changes
- Code formatting changes
- Merge commits

```
docs: Update README [skip ci]
style: Format code [no ci]
```

## Creating Commits

### Using Git Directly

```bash
# Stage changes
git add -A

# Create commit with proper format
git commit -m "feat(bot): Add new command"

# Or use full format with body
git commit -m "feat(bot): Add new command" -m "Add description of changes"
```

### Commit Message Validation

Before committing, verify:
- [ ] Type is correct (feat, fix, refactor, etc.)
- [ ] Scope is appropriate (bot, db, voice, feed, update)
- [ ] Subject uses imperative mood
- [ ] Documentation-only commits have `[skip ci]` or similar

## Common Mistakes

| Mistake | Correction |
|---------|------------|
| `Added new feature` | `feat(bot): Add new feature` |
| `fix(bot): fixed bug` | `fix(bot): Fix bug` |
| `refactor: Refactored code` | `refactor(bot): Refactor code structure` |

## Git Log Reference

Recent commits in this project follow these patterns:
```
23c0370 fix(bot): `/vc leaderboard` instant timeout when initial data is empty
f597bb6 docs: update docs
f7a25b4 refactor: remove unused variables
e8fe0dd refactor: separate model & update logic of voice leaderboard
6aa5545 docs: rename docs [no ci]
```
