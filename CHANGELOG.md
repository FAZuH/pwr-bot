# Changelog

## [Unreleased]

### Added

- Added the settings hub, one panel for feed, voice, and welcome settings
- Added `/plugins` to manage the bot's plugins
- Feed, voice, and welcome settings now open from the settings hub
- Added a live preview card to welcome settings
- Added a confirmation dialog before welcome messages are removed
- The About panel now shows live server statistics
- Back now returns to the settings hub from every panel
- Bot panels now use Discord's newest message components
- Panel views now open in the same message instead of posting a new one

### Fixed

- `!register` no longer fails when a core plugin is also listed in the catalog
- Fixed settings hub failing with an internal error
- Fixed gateway failing to start when both rustls providers are linked
- Fixed bot panels failing to update after an action
- Fixed plugin commands timing out before answering
- Fixed commands hanging with a thinking spinner after an error
- Fixed silent failures when a plugin fails to load
