#!/bin/sh
# Test-only plugin fixture: announces a valid hello, then ignores every
# message (including bye) and never exits on its own — even on stdin EOF, the
# read fails and the loop retries after a sleep. stop() must fall back to
# signaling its process group: SIGTERM first, then SIGKILL on a second grace
# timeout, and the host must observe a signal death.
echo '{"t":"hello","v":1,"name":"stubborn","caps":[]}'
while true; do
    IFS= read -r line || sleep 1
done