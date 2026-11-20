-- Per-person limits on adding content, next to the existing community limits (admins can change
-- them at /admin/community-settings). Moderators and admins are not limited.
UPDATE settings
   SET value = jsonb_set(value, '{rate}', (value -> 'rate')
        || '{"contribute": {"max": 30, "window_secs": 3600}, "upload": {"max": 20, "window_secs": 3600}}'::jsonb)
 WHERE key = 'community';
