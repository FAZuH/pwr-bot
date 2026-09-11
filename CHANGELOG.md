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

### Fixed

- Fixed bot panels failing to update after an action
- Fixed plugin commands timing out before answering
- Fixed commands hanging with a thinking spinner after an error
- Fixed silent failures when a plugin fails to load
