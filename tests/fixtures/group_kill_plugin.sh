#!/bin/sh
# Test-only plugin fixture: announces a valid hello, then spawns a child
# subprocess into its own process group; both the plugin and the child
# ignore SIGTERM, and the loop never reads stdin, so bye/EOF and the group
# SIGTERM all fail to stop it — unload must escalate to the group SIGKILL.
# The child pid is written to a marker file; because the child ignores
# SIGTERM, its death after unload can only come from the group SIGKILL.
echo '{"t":"hello","v":1,"name":"group-kill","caps":[]}'
BASE="/tmp/pwr_bot_plugin_group_${PPID}_kill"
trap '' TERM
sh -c 'trap "" TERM; while :; do sleep 1; done' &
echo $! > "$BASE.pid"
while :; do sleep 1; done