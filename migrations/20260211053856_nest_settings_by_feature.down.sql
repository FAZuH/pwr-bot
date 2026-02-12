-- Add down migration script here
UPDATE server_settings
SET settings = json_object(
    'enabled', json_extract(settings, '$.feeds.enabled'),
    'channel_id', json_extract(settings, '$.feeds.channel_id'),
    'subscribe_role_id', json_extract(settings, '$.feeds.subscribe_role_id'),
    'unsubscribe_role_id', json_extract(settings, '$.feeds.unsubscribe_role_id'),
    'voice_tracking_enabled', json_extract(settings, '$.voice.enabled')
);
