## [0.1.12](https://github.com/FAZuH/pwr-bot/compare/v0.1.11...v0.1.12) (2025-12-29)


### New Features

* **bot:** Add enabled option in feed /settings ([f41e8a2](https://github.com/FAZuH/pwr-bot/commit/f41e8a2599950c5b122e1250640122897ae33524))
* **bot:** Add permission checks on feeds commands ([98dfa93](https://github.com/FAZuH/pwr-bot/commit/98dfa93437a905f7dfbf35638b1c4c0ef79f3366))
* **bot:** Add role settings for subscriptions ([895ad1e](https://github.com/FAZuH/pwr-bot/commit/895ad1eacd7f0dd5c8730a84a6c113f792096ab3))
* **bot:** Add settings command and update target id generation ([b556af9](https://github.com/FAZuH/pwr-bot/commit/b556af9a824ac98e9d62a837676a0641c2ca80e0))
* **db:** Add server_settings table and update subscriber logic ([1cd1e62](https://github.com/FAZuH/pwr-bot/commit/1cd1e625fc59a34f3e2538b016166fb670534e74))
* **service:** Implement server settings business logic ([1fd1076](https://github.com/FAZuH/pwr-bot/commit/1fd1076337951b00ae67b609ca6097918e7d158b))


### Bug Fixes

* Fix /unsubscribe autocomplete not showing DM subscriptions when in a guild [skip ci] ([d6bbdac](https://github.com/FAZuH/pwr-bot/commit/d6bbdac0f44072d71339a4d2709578fc623829c0))
* **publisher:** Fix incorrect format on published message ([e80841f](https://github.com/FAZuH/pwr-bot/commit/e80841fd6599df18e0107849e6fd84045927ffdb))


### Performance Improvements

* **bot:** Improve bot initialization [skip ci] ([e055f1d](https://github.com/FAZuH/pwr-bot/commit/e055f1d64224027dc0366a8f8a3f415b71c68a0f))


### Styles

* rustfmt formatting ([128cb4c](https://github.com/FAZuH/pwr-bot/commit/128cb4c3e56f6b7f0238cd05e30cd9ae13236cfa))


### Miscellaneous Chores

* Add elapsed time to setup logs [skip ci] ([d2b4d87](https://github.com/FAZuH/pwr-bot/commit/d2b4d87302433d86cc7a8272cd306ae1449052b0))
* Misc ([fa4864a](https://github.com/FAZuH/pwr-bot/commit/fa4864a4ca310fc2bcf82ab653b523861baab5c0))
* Remove dead code ([d2b3ede](https://github.com/FAZuH/pwr-bot/commit/d2b3edef0192b206e8bcc6fced3df654cf557cb4))


### Code Refactoring

* **bot:** Organize FeedsCog functions ([64d5afb](https://github.com/FAZuH/pwr-bot/commit/64d5afb20ce1470f5663f415ade8c3b506f8fd3b))
* **db:** Store guild_id as integer instead of string ([69ce299](https://github.com/FAZuH/pwr-bot/commit/69ce299d92cc1416a886acd64ebb3715c7854319))
* Generalize series feed [skip ci] ([#13](https://github.com/FAZuH/pwr-bot/issues/13)) ([29a3e94](https://github.com/FAZuH/pwr-bot/commit/29a3e949d9fd8978330f018b964eaa78d19debb9))
* Organize commands ([#12](https://github.com/FAZuH/pwr-bot/issues/12)) [skip ci] ([c455003](https://github.com/FAZuH/pwr-bot/commit/c4550038909e60c074c20e339295e766a324894c))


### Tests

* Add test_feed_interval_calculation [skip ci] ([402f317](https://github.com/FAZuH/pwr-bot/commit/402f317a57348edee45601a69dff09bc75d16a6b))
* Add tests [skip ci] ([#11](https://github.com/FAZuH/pwr-bot/issues/11)) ([ad71d4e](https://github.com/FAZuH/pwr-bot/commit/ad71d4e7f1c6fa741136823962ee44b74279acc8))
* **bot:** Add unit tests for FeedsCog ([f6907ec](https://github.com/FAZuH/pwr-bot/commit/f6907eccb732bcfb6f1e0f191e223fb146354d79))
* **db:** Improve db table tests ([9ebb0b5](https://github.com/FAZuH/pwr-bot/commit/9ebb0b5848e2e816b78339b504404cd317a96f33))
* Update db table and service tests for server settings ([121b957](https://github.com/FAZuH/pwr-bot/commit/121b957eead1853626ae06c10748c88c76d76598))

## [0.1.11](https://github.com/FAZuH/pwr-bot/compare/v0.1.10...v0.1.11) (2025-12-28)


### Miscellaneous Chores

* **release:** v0.1.11 [skip ci] ([bb5128e](https://github.com/FAZuH/pwr-bot/commit/bb5128e9cc49ba7bb5089577b96be0e444f9f1dd))


### Code Refactoring

* **db:** Use macros for TableBase and Table impls ([95d03f2](https://github.com/FAZuH/pwr-bot/commit/95d03f2479e57554b88f643f27370c8d9abbec68))

## [0.1.10](https://github.com/FAZuH/pwr-bot/compare/v0.1.9...v0.1.10) (2025-12-28)


### Performance Improvements

* Improve API requests and database queries ([#10](https://github.com/FAZuH/pwr-bot/issues/10)) ([4cc9e0d](https://github.com/FAZuH/pwr-bot/commit/4cc9e0d5715c8ab355dca394df34d10ffb896228))


### Miscellaneous Chores

* **release:** v0.1.10 [skip ci] ([bcdf33b](https://github.com/FAZuH/pwr-bot/commit/bcdf33b9b1747a39afd9d92c24f9d89323f9dcc8))

## [0.1.9](https://github.com/FAZuH/pwr-bot/compare/v0.1.8...v0.1.9) (2025-12-28)


### Miscellaneous Chores

* **release:** v0.1.9 [skip ci] ([cef1a79](https://github.com/FAZuH/pwr-bot/commit/cef1a7947f4edc28b19b1fed96aa9a2a3630bc2e))


### Code Refactoring

* Various ([#9](https://github.com/FAZuH/pwr-bot/issues/9)) ([00d49e5](https://github.com/FAZuH/pwr-bot/commit/00d49e586679f86833a5a2f55fe3c5bfed45ad81))

## [0.1.8](https://github.com/FAZuH/pwr-bot/compare/v0.1.7...v0.1.8) (2025-12-26)


### Miscellaneous Chores

* **release:** v0.1.8 [skip ci] ([da3c37c](https://github.com/FAZuH/pwr-bot/commit/da3c37c1c829bdcd69098d51c9444e9c16dfd87a))


### Build System

* Fix and improve build ([#5](https://github.com/FAZuH/pwr-bot/issues/5)) ([a297a52](https://github.com/FAZuH/pwr-bot/commit/a297a52383d56556eb2a2e282e8e44fe15d8320c))

