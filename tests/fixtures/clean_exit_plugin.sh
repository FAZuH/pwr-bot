#!/bin/sh
# Test-only plugin fixture: announces a valid hello, then exits 0 on its own
# shortly after — a clean, intentional exit with no bye and no kill. The
# health task must treat exit 0 as intentional: remove the entry without
# respawning.
echo '{"t":"hello","v":2,"name":"clean","ops":[]}'
sleep 0.3
exit 0