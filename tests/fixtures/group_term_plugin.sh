#!/bin/sh
# Test-only plugin fixture: announces a valid hello, then spawns a child
# subprocess into its own process group and ignores bye/EOF forever (the loop
# never reads stdin), so unload must fall through to the group SIGTERM. The
# plugin itself has no TERM trap and dies on SIGTERM like any default
# handler; the child records SIGTERM receipt into a marker file so the test
# can assert the group signal reached it. Marker/pid paths derive from the
# parent test process id (stable per test binary) plus a fixture tag, which
# the test mirrors.
echo '{"t":"hello","v":1,"name":"group-term","caps":[]}'
BASE="/tmp/pwr_bot_plugin_group_${PPID}_term"
sh -c 'trap "echo term > \"$1\"; exit 0" TERM; while :; do sleep 1; done' sh "$BASE.marker" &
echo $! > "$BASE.pid"
while :; do sleep 1; done