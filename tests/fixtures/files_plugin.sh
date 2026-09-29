#!/bin/sh

echo '{"t":"hello","v":2,"name":"files","ops":[]}'
while IFS= read -r line; do
    case "$line" in
        *'"t":"bye"'*)
            exit 0
            ;;
        *'"op":"invoke"'*)
            id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
            if printf '%s' "$line" | grep -q '"cmd":"malformed"'; then
                printf '{"t":"resp","id":%s,"ok":true,"data":{"data":{"content":null,"nonce":"files","tts":false,"embeds":[],"allowed_mentions":{"parse":[]},"message_reference":null,"components":[],"sticker_ids":[],"flags":32768,"attachments":[{"id":0,"filename":"preview.png"}],"enforce_nonce":false,"poll":null},"ephemeral":false,"view":null,"files":[{"filename":"runtime.bin","data_base64":"not-base64"}]}}\n' "$id"
            else
                printf '{"t":"resp","id":%s,"ok":true,"data":{"data":{"content":null,"nonce":"files","tts":false,"embeds":[],"allowed_mentions":{"parse":[]},"message_reference":null,"components":[],"sticker_ids":[],"flags":32768,"attachments":[{"id":0,"filename":"preview.png"}],"enforce_nonce":false,"poll":null},"ephemeral":false,"view":null,"files":[{"filename":"runtime.bin","data_base64":"aGk="}]}}\n' "$id"
            fi
            ;;
    esac
done
