#!/bin/sh

# Reports what of the host's environment crossed the process seam through
# its exit code, so a test can judge the allowlist: 0 nothing inherited,
# 42 DISCORD_TOKEN visible (the operator's grant), 43 another host
# variable leaked.
host_env_audit() {
    for name in DB_URL DISCORD_APPLICATION_ID ADMIN_ID; do
        eval "[ \"\${$name+x}\" = x ]" && return 43
    done
    [ "${DISCORD_TOKEN+x}" = x ] && return 42
    return 0
}

echo '{"t":"hello","v":2,"name":"env-probe","ops":[]}'
while IFS= read -r line; do
    case "$line" in
        *'"t":"bye"'*)
            host_env_audit
            exit $?
            ;;
    esac
done
