#!/bin/sh
# Test-only plugin fixture: announces a valid hello, then never answers any
# call (leaving it in flight), but exits 0 on `bye` or stdin EOF like the
# canonical fixture. Unloading while a call is in flight must fail the call
# with a PluginDied wire error rather than hang it.
echo '{"t":"hello","v":1,"name":"hung","caps":[]}'
while IFS= read -r line; do
    case "$line" in
        *'"t":"bye"'*) exit 0 ;;
    esac
done
exit 0