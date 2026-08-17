# Changelog Guide

The `CHANGELOG.md` structure follows
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/). Read that
spec first; this guide adds the house style on top of it.

The release workflow does not generate the changelog. On release, the topmost
`## [Unreleased]` section is renamed to `## <version> (<date>)` — e.g.
`## 0.1.3 (2026-08-14)` — and a fresh empty `## [Unreleased]` is inserted
above it.

## What belongs in the changelog

Only user-visible changes. A change belongs when it alters how the software
behaves for the user: a new feature, a changed behavior, a fixed bug, a
removed capability.

Internal work stays out: refactors, dependency bumps, tooling, CI, test, and
documentation changes. When in doubt, leave it out.

## Wording

Write each entry as a plain statement about user-visible behavior. Past
tense. Short, concrete sentences. No implementation detail, no jargon, no
commit hashes, no issue or pull-request links.

Examples:

```
- Fixed the Golden Avia flying away from you under certain conditions
- Added up to level 25 Potions of Healing to the Elkurn Potion Merchant
- Reduced the health and damage of all enemies, including the boss
- Removed the Cindercurse set pieces from the normal loot pool
- Moderately reduced the health and damage of all enemies, including the boss
- Massively reduced the knockback of all enemy attacks, including the boss
- Enemies stuck in the water will instead die
- Fixed the boss having an abnormally high walk speed
- Fixed enemies not having any animations on spawn
- Fixed a technical issue which would sometimes prevent people from progressing
- Altered the music by having the fight music play throughout the sequence instead
```

## Brevity

The examples above are the ceiling for entry length: one short clause,
under 15 words. State the outcome, not the mechanism. Cut any clause that
explains how, why, or what it prevents. If the entry still reads correctly
without a clause, that clause was detail.

## Grammar rules

- Start each entry with the change word: `Added`, `Changed`, `Fixed`,
  `Removed`, `Reduced`, `Adjusted`, `Updated`.
- Name the change first, then what changed about it. Do not explain how it
  works or why.
- Keep one idea per entry. Split long entries into several bullets.
- Do not use "should", "may", "via", "e.g.", "i.e.", or "etc.".
- Do not start with filler such as "Support has been added for...". Say
  "Added ...".

## Grouping

Use the Keep a Changelog subheadings under `## [Unreleased]`: `### Added`,
`### Changed`, `### Fixed`, `### Removed`, and `### Breaking Changes` (first,
when present). Omit a subheading that has no entries. Put related bullets
together under the same subheading.
