-- Page addresses are always in Latin letters, made from the English title. (An operator can turn
-- native-script addresses back on with slug.preserve_native_script.) Every page is re-sluged by the
-- worker; the old addresses stay as history and redirect to the new ones.
UPDATE settings
   SET value = jsonb_set(value, '{slug,preserve_native_script}', 'false'::jsonb)
 WHERE key = 'seo';

INSERT INTO outbox (topic, aggregate_type, aggregate_id, payload)
SELECT 'search.reindex', t, id, '{}' FROM (
    SELECT 'composition' AS t, id FROM compositions UNION ALL
    SELECT 'recording', id FROM recordings UNION ALL
    SELECT 'artist', id FROM artists UNION ALL
    SELECT 'album', id FROM albums) x;
