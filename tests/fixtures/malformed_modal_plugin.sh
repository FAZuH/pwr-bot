#!/bin/sh

echo '{"t":"hello","v":2,"name":"malformed-modal","ops":[]}'
while IFS= read -r line; do
    case "$line" in
        *modal_submit*)
            id=$(printf '%s' "$line" | sed -n 's/^{"t":"call","id":\([0-9][0-9]*\).*/\1/p')
            printf '{"t":"resp","id":%s,"ok":true,"data":{"data":{"content":"modal"},"ephemeral":false,"view":{"modal":true},"files":[{"filename":"broken.bin","data_base64":"not base64"}]}}\n' "$id"
            ;;
        *view.interact*)
            id=$(printf '%s' "$line" | sed -n 's/^{"t":"call","id":\([0-9][0-9]*\).*/\1/p')
            printf '{"t":"resp","id":%s,"ok":true,"data":{"data":{"content":"fallback committed"},"ephemeral":false,"view":{"fallback":true},"files":[]}}\n' "$id"
            ;;
        *'"t":"bye"'*)
            exit 0
            ;;
    esac
done
