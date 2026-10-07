## 0.4.2 (2026-10-05)

## 0.4.1 (2026-10-05)

## [Unreleased]

### feat

* **plugin:** rename feature plugins to feed, voice, welcome ([b83e48e](https://github.com/FAZuH/pwr-bot/commit/b83e48e30e731eea05b01f4be5d5768f9ade57d1))
* **plugin:** host Settings capability, raw sends, retire the hub plugin ([192d5f7](https://github.com/FAZuH/pwr-bot/commit/192d5f7e7285dbc27a3ccb1fff03d49f9ff6971f))
* **plugin:** move the feed feature into the feed plugin ([30354ee](https://github.com/FAZuH/pwr-bot/commit/30354ee442fcd69bbc6eda34457449861ddd5d43))
* **plugin:** extract voice features into a plugin ([2c46292](https://github.com/FAZuH/pwr-bot/commit/2c4629273838d5693963cbcc0dd70148124b29fb))
* **plugin:** grant the Discord token to a pinned catalog plugin ([aa0b4cf](https://github.com/FAZuH/pwr-bot/commit/aa0b4cff33fe33914d7211d3d7a5cfbc18115b44))
* **plugin:** extract the welcome plugin and split server_settings ([bb512c5](https://github.com/FAZuH/pwr-bot/commit/bb512c53b6b38c13ac10800617d328ac7860fa8e))
* **host:** remove every plugin-specific trace from the host ([e919779](https://github.com/FAZuH/pwr-bot/commit/e919779d46f83c636c4aeef64a5629b4bb5c12fc))
* **host:** one /settings command, admin-gated, with panel autocomplete ([1890a91](https://github.com/FAZuH/pwr-bot/commit/1890a91069634372ecd5bd37af9471abab922067))
* **plugin:** let a guild toggle a core plugin, and honour it ([c2a0cad](https://github.com/FAZuH/pwr-bot/commit/c2a0caddbdfff7b7ff98fb1277bf3b082fc9d10a))
* **plugins:** list every plugin as a view, with a Show/Hide Internal toggle ([9014960](https://github.com/FAZuH/pwr-bot/commit/90149600f4076b71260f8fe1a36241a5cc84ccfb))

### fix

* **plugin:** persist settings toggles under per-feature KV keys ([556fa39](https://github.com/FAZuH/pwr-bot/commit/556fa395cd85c56b98968f466d9a23fa5d84f451))
* **plugin:** restore the panels' About button ([ecd5dcc](https://github.com/FAZuH/pwr-bot/commit/ecd5dccc4cde98dac760c5efd52bb086d8dc0ff3))
* **ci:** run the workspace suite in the PR gate ([e8b6928](https://github.com/FAZuH/pwr-bot/commit/e8b69283cbc841448b523598156eb73662566f41))
* **plugin:** close voice extraction review gaps ([8be02aa](https://github.com/FAZuH/pwr-bot/commit/8be02aa755bd39af4f533b96852bbd7b621572f8))
* build the whole workspace in the Docker app stage ([e3796bb](https://github.com/FAZuH/pwr-bot/commit/e3796bbf218e5e128659cf64eebf495c639a1a4a))
* build the plugin fixtures unconditionally in probe tests ([24ee8be](https://github.com/FAZuH/pwr-bot/commit/24ee8be0706a51d1637652d5cdb62b9b4c894406))
* **deps:** migrate yanked wreq 5.x line to wreq 0.16 ([a700982](https://github.com/FAZuH/pwr-bot/commit/a7009823e561da1f4c99619516fe6cec981bcd79))
* **deps:** follow poise command field-to-method rename ([34e1fde](https://github.com/FAZuH/pwr-bot/commit/34e1fdef43180d391838317441c1bf5a9cb03522))
* **plugin:** gate the settings handoff, and stop /plugins list 500ing ([5f820f2](https://github.com/FAZuH/pwr-bot/commit/5f820f2601f0cdfc557763a6f0fc2265d8e1be76))
* **settings:** give the root an About button, and take Back off it ([353356f](https://github.com/FAZuH/pwr-bot/commit/353356f14bc815cacdc08ad29c2c00ab5c884b84))
* **settings:** label the root's About button with the glyph the plugins already use ([639b93a](https://github.com/FAZuH/pwr-bot/commit/639b93ad30de660f3f685389cd2fa871ccf5ce1c))
* **docker:** install ca-certificates in the runtime stage ([a8f1690](https://github.com/FAZuH/pwr-bot/commit/a8f1690ea934b4343342d6713998592e5ed637ed))

## 0.4.0 (2026-09-16)

## [0.3.2](https://github.com/FAZuH/pwr-bot/compare/v0.3.1...v0.3.2) (2026-04-30)

## [0.3.1](https://github.com/FAZuH/pwr-bot/compare/v0.3.0...v0.3.1) (2026-04-30)


### fix

* Bot status showing old version ([76669cb](https://github.com/FAZuH/pwr-bot/commit/76669cb795798a313ca672ac2d045a2fe3b98d29))
* **bot:** Inflated voice leaderboard sessions ([548f62d](https://github.com/FAZuH/pwr-bot/commit/548f62d94860479a5e05afe3b598c2bd44935188))
* **bot:** Internal Error on `/vc leaderboard` ([e4782ec](https://github.com/FAZuH/pwr-bot/commit/e4782ec21b50280c80851099cae2e2b04609b9eb))

## [0.3.0](https://github.com/FAZuH/pwr-bot/compare/v0.2.19...v0.3.0) (2026-04-29)


### ⚠ BREAKING CHANGES

* Migrate db backend to PostgreSQL [pub]

### feat

* Add db migration script ([573c96f](https://github.com/FAZuH/pwr-bot/commit/573c96f8b22fc65059511c29a5f475295662ee8d))


### Code Refactoring

* Migrate db backend to PostgreSQL ([2c7546b](https://github.com/FAZuH/pwr-bot/commit/2c7546b561d25cb33982867adca02b795609e308))

## [0.2.19](https://github.com/FAZuH/pwr-bot/compare/v0.2.18...v0.2.19) (2026-04-29)


### feat

* **bot:** Improve /gui_test UI ([6006d73](https://github.com/FAZuH/pwr-bot/commit/6006d73d87644d078f74c2090efd9a93f296ff4d))


### fix

* **bot:** `/vc leaderboard` instant timeout when initial data is empty ([23c0370](https://github.com/FAZuH/pwr-bot/commit/23c0370019ebdc6ad4dcca4be04e39f55ad6ee61))

## [0.2.18](https://github.com/FAZuH/pwr-bot/compare/v0.2.17...v0.2.18) (2026-04-28)