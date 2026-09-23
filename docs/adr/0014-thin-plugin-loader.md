# Thin plugin loader core

By v0.5 the `pwr-bot` core crate is a thin plugin loader and nothing else:
plugin spawn and supervision, the wire protocol, the manifest-to-poise
command bridge, the view/modal engine, plugin kv, plugin management
(`/plugins`), and the Settings capability. All concrete functionality —
feed, voice, welcome — ships as plugin subprocesses named `feed`, `voice`,
and `welcome`, and no module under `src/` imports or names any of them.
The alternative was growing typed domain RPCs in the host (the ADR-0010
pattern the settings panels used), but that keeps the host permanently
domain-aware and turns every query into protocol surface; the loader goal
rules it out. Supersedes the panel-era framing of ADR-0009 (panels are no
longer the only thing pluginized — the whole feature set is); ADR-0013's
kept gui runtime now hosts the Settings capability instead of monolith
features.
