#!/bin/sh
# Test-only plugin fixture: announces a valid hello, then kills itself with
# SIGKILL upon receiving the second message (the host's hello ack is the
# first). Any call the host has in flight at that point dies with it, and the
# host must observe a signal death rather than a clean exit. The stderr line
# right after the hello is a deterministic log the host must forward (see
# plugin_stderr_reaches_host_logs).
echo '{"t":"hello","v":1,"name":"crash","caps":[]}'
echo "crash_plugin: starting" >&2
count=0
while IFS= read -r line; do
    count=$((count + 1))
    if [ "$count" -ge 2 ]; then
        kill -9 $$
    fi
done
