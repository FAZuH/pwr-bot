#!/bin/sh

echo '{"t":"hello","v":2,"name":"env-probe","ops":[]}'
while IFS= read -r line; do
    case "$line" in
        *'"t":"bye"'*)
            if [ "${DISCORD_TOKEN+x}" = x ]; then
                exit 42
            fi
            exit 0
            ;;
    esac
done
