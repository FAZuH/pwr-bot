#!/bin/sh
# Test-only plugin fixture: a `CORE_PLUGINS` stand-in whose hello manifest
# declares the Discord token need. A core plugin is never grantable, so the
# host must refuse it at the handshake seam rather than let it run without
# the authority it declared (ADR-0016).
echo '{"t":"hello","v":2,"name":"token-need-probe","ops":[],"manifest":{"name":"token-need-probe","description":"declares the token need","version":"0.1.0","commands":[],"event_handlers":[],"tasks":[],"settings":[],"requires":["discord_token"],"api_version":2}}'
while IFS= read -r line; do
    case "$line" in
        *'"t":"bye"'*)
            exit 0
            ;;
    esac
done
exit 0
