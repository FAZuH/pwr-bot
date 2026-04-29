-- Add up migration script here
UPDATE server_settings
SET settings = json_object(
    'feeds', json_object(
        'enabled', json_extract(settings, '$.enabled'),
        'channel_id', json_extract(settings, '$.channel_id'),
        'subscribe_role_id', json_extract(settings, '$.subscribe_role_id'),
        'unsubscribe_role_id', json_extract(settings, '$.unsubscribe_role_id')
    ),
    'voice', json_object(
        'enabled', json_extract(settings, '$.voice_tracking_enabled')
    )
);
