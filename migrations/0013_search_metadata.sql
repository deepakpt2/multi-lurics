-- Search by raga, deity and tag. They are indexed as extra searchable text of a composition (origin
-- 'metadata'); the result still shows the song, with a "matched via" badge. Forward-only: the refresh
-- function is replaced, the trigger also fires on the new columns, and existing songs are re-queued.
ALTER TABLE search_documents DROP CONSTRAINT search_documents_origin_check;
ALTER TABLE search_documents ADD CONSTRAINT search_documents_origin_check
    CHECK (origin IN ('name', 'localization', 'transliteration', 'alias', 'metadata'));

CREATE OR REPLACE FUNCTION search_documents_refresh(t text, eid uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE canonical text; pop integer; writer boolean;
BEGIN
    DELETE FROM search_documents WHERE entity_type = t AND entity_id = eid;
    canonical := CASE t
        WHEN 'artist'      THEN (SELECT name  FROM artists      WHERE id = eid)
        WHEN 'album'       THEN (SELECT title FROM albums       WHERE id = eid)
        WHEN 'composition' THEN (SELECT title FROM compositions WHERE id = eid)
        WHEN 'recording'   THEN (SELECT title FROM recordings   WHERE id = eid)
    END;
    IF canonical IS NULL THEN RETURN; END IF;   -- deleted (for example merged away)

    pop := CASE t
        WHEN 'recording' THEN (SELECT count(*) FROM contributions
                               WHERE recording_id = eid AND published_revision_id IS NOT NULL)
        WHEN 'composition' THEN (SELECT count(*) FROM contributions c JOIN recordings r ON r.id = c.recording_id
                                 WHERE r.composition_id = eid AND c.published_revision_id IS NOT NULL)
        WHEN 'album' THEN (SELECT count(*) FROM contributions c JOIN album_tracks a ON a.recording_id = c.recording_id
                           WHERE a.album_id = eid AND c.published_revision_id IS NOT NULL)
        WHEN 'artist' THEN (SELECT count(*) FROM contributions c JOIN recordings r ON r.id = c.recording_id
                            WHERE c.published_revision_id IS NOT NULL AND EXISTS (
                                SELECT 1 FROM credits x WHERE x.artist_id = eid
                                  AND (x.recording_id = r.id OR x.composition_id = r.composition_id)))
    END;
    writer := t = 'artist' AND EXISTS (SELECT 1 FROM credits WHERE artist_id = eid AND composition_id IS NOT NULL);

    INSERT INTO search_documents (entity_type, entity_id, key, compact, display, origin, language_id, script_id,
                                  source_id, popularity, contributor)
    SELECT t, eid, k.key, replace(k.key, ' ', ''), s.disp, s.o, s.lang, s.scr, s.src, pop, writer
    FROM (
        SELECT 1 AS prio, 'name' AS o, canonical AS disp, NULL::uuid AS lang, NULL::uuid AS scr, NULL::uuid AS src
        UNION ALL
        SELECT 2, CASE WHEN provenance = 'transliteration' THEN 'transliteration' ELSE 'localization' END,
               name, language_id, script_id, id
        FROM entity_localizations
        WHERE entity_type = t AND entity_id = eid AND verification_state = 'verified'
        UNION ALL
        SELECT 3, 'alias', alias, language_id, script_id, id
        FROM entity_aliases
        WHERE entity_type = t AND entity_id = eid AND verification_state = 'verified'
        UNION ALL
        -- Raga, deity and tags make a song findable ("Hanuman", "Mohanam", "bhajan").
        SELECT 4, 'metadata', v, NULL::uuid, NULL::uuid, NULL::uuid
        FROM compositions c, LATERAL unnest(ARRAY[c.raga, c.deity] || c.tags) AS v
        WHERE t = 'composition' AND c.id = eid AND v IS NOT NULL
    ) s
    CROSS JOIN LATERAL (SELECT lyrics_search_key(s.disp) AS key) k
    WHERE k.key <> ''
    ORDER BY s.prio
    ON CONFLICT DO NOTHING;
END $$;

DROP TRIGGER compositions_search_docs ON compositions;
CREATE TRIGGER compositions_search_docs AFTER INSERT OR UPDATE OF title, raga, deity, tags OR DELETE ON compositions
    FOR EACH ROW EXECUTE FUNCTION search_documents_entity_trigger('composition');

SELECT search_enqueue('composition', id) FROM compositions;
