-- Public search (Phase 6): a published-only read model, kept current by the outbox worker.
--
-- `search_documents` holds one row per searchable name of a composition, recording, artist or
-- album: the canonical name plus verified localizations, transliterations and aliases. Pending
-- or rejected names are never written. Lyric bodies are never indexed. Rows are rebuilt from the
-- source tables by `search_documents_refresh`, which is idempotent, so redelivered events are safe.
-- Normalization reuses `lyrics_search_key` (0006): one definition of "the same text".

CREATE TABLE search_documents (
    entity_type text    NOT NULL CHECK (entity_type IN ('composition', 'recording', 'artist', 'album')),
    entity_id   uuid    NOT NULL,
    key         text    NOT NULL,
    compact     text    NOT NULL,
    display     text    NOT NULL,   -- as written, never normalized
    origin      text    NOT NULL CHECK (origin IN ('name', 'localization', 'transliteration', 'alias')),
    language_id uuid    NULL,
    script_id   uuid    NULL,
    -- The localization or alias row this came from: queries re-check it is still verified, so an
    -- unverified name disappears at once even before the worker has rebuilt the entity.
    source_id   uuid    NULL,
    popularity  integer NOT NULL DEFAULT 0 CHECK (popularity >= 0),   -- published lyric contributions
    contributor boolean NOT NULL DEFAULT false,                       -- credited as writer/translator
    PRIMARY KEY (entity_type, entity_id, key)
);
CREATE INDEX search_documents_key_trgm ON search_documents USING gin (key gin_trgm_ops);
CREATE INDEX search_documents_compact_idx ON search_documents (entity_type, compact text_pattern_ops);

CREATE FUNCTION search_documents_refresh(t text, eid uuid) RETURNS void LANGUAGE plpgsql AS $$
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
    ) s
    CROSS JOIN LATERAL (SELECT lyrics_search_key(s.disp) AS key) k
    WHERE k.key <> ''
    ORDER BY s.prio
    ON CONFLICT DO NOTHING;
END $$;

-- ---- change capture: every write path enqueues a reindex in its own transaction ----
-- One pending (never attempted) event per entity is enough: the refresh reads current state.
CREATE UNIQUE INDEX outbox_search_pending_key ON outbox (aggregate_type, aggregate_id)
    WHERE topic = 'search.reindex' AND processed_at IS NULL AND attempts = 0;

CREATE FUNCTION search_enqueue(t text, eid uuid) RETURNS void LANGUAGE sql AS $$
    INSERT INTO outbox (topic, aggregate_type, aggregate_id, payload) VALUES ('search.reindex', t, eid, '{}')
    ON CONFLICT (aggregate_type, aggregate_id)
        WHERE topic = 'search.reindex' AND processed_at IS NULL AND attempts = 0 DO NOTHING
$$;

-- A recording's lyrics changed: its own popularity, its composition's, its credited artists' and albums'.
CREATE FUNCTION search_enqueue_recording(rid uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE cid uuid;
BEGIN
    SELECT composition_id INTO cid FROM recordings WHERE id = rid;
    IF NOT FOUND THEN RETURN; END IF;
    PERFORM search_enqueue('recording', rid);
    PERFORM search_enqueue('composition', cid);
    PERFORM search_enqueue('artist', artist_id) FROM credits WHERE recording_id = rid OR composition_id = cid;
    PERFORM search_enqueue('album', album_id) FROM album_tracks WHERE recording_id = rid;
END $$;

CREATE FUNCTION search_documents_entity_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM search_enqueue(TG_ARGV[0], OLD.id);
    ELSE
        PERFORM search_enqueue(TG_ARGV[0], NEW.id);
    END IF;
    RETURN NULL;
END $$;

CREATE TRIGGER artists_search_docs AFTER INSERT OR UPDATE OF name OR DELETE ON artists
    FOR EACH ROW EXECUTE FUNCTION search_documents_entity_trigger('artist');
CREATE TRIGGER albums_search_docs AFTER INSERT OR UPDATE OF title OR DELETE ON albums
    FOR EACH ROW EXECUTE FUNCTION search_documents_entity_trigger('album');
CREATE TRIGGER compositions_search_docs AFTER INSERT OR UPDATE OF title OR DELETE ON compositions
    FOR EACH ROW EXECUTE FUNCTION search_documents_entity_trigger('composition');
CREATE TRIGGER recordings_search_docs AFTER INSERT OR UPDATE OF title, composition_id OR DELETE ON recordings
    FOR EACH ROW EXECUTE FUNCTION search_documents_entity_trigger('recording');

CREATE FUNCTION search_documents_child_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'DELETE' AND NEW.entity_type IN ('composition', 'recording', 'artist', 'album') THEN
        PERFORM search_enqueue(NEW.entity_type, NEW.entity_id);
    END IF;
    IF TG_OP <> 'INSERT' AND OLD.entity_type IN ('composition', 'recording', 'artist', 'album') THEN
        PERFORM search_enqueue(OLD.entity_type, OLD.entity_id);
    END IF;
    RETURN NULL;
END $$;
CREATE TRIGGER localizations_search_docs AFTER INSERT OR UPDATE OR DELETE ON entity_localizations
    FOR EACH ROW EXECUTE FUNCTION search_documents_child_trigger();
CREATE TRIGGER aliases_search_docs AFTER INSERT OR UPDATE OR DELETE ON entity_aliases
    FOR EACH ROW EXECUTE FUNCTION search_documents_child_trigger();

-- Credits and album tracks change an artist's/album's popularity and writer flag (this also covers merges).
CREATE FUNCTION search_documents_credit_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'DELETE' THEN PERFORM search_enqueue('artist', NEW.artist_id); END IF;
    IF TG_OP <> 'INSERT' THEN PERFORM search_enqueue('artist', OLD.artist_id); END IF;
    RETURN NULL;
END $$;
CREATE TRIGGER credits_search_docs AFTER INSERT OR UPDATE OR DELETE ON credits
    FOR EACH ROW EXECUTE FUNCTION search_documents_credit_trigger();

CREATE FUNCTION search_documents_track_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'DELETE' THEN PERFORM search_enqueue('album', NEW.album_id); END IF;
    IF TG_OP <> 'INSERT' THEN PERFORM search_enqueue('album', OLD.album_id); END IF;
    RETURN NULL;
END $$;
CREATE TRIGGER album_tracks_search_docs AFTER INSERT OR UPDATE OR DELETE ON album_tracks
    FOR EACH ROW EXECUTE FUNCTION search_documents_track_trigger();

-- Publishing or unpublishing lyrics (moderation) moves popularity.
CREATE FUNCTION search_documents_contribution_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' OR NEW.published_revision_id IS DISTINCT FROM OLD.published_revision_id THEN
        PERFORM search_enqueue_recording(NEW.recording_id);
    END IF;
    RETURN NULL;
END $$;
CREATE TRIGGER contributions_search_docs AFTER INSERT OR UPDATE OF published_revision_id ON contributions
    FOR EACH ROW EXECUTE FUNCTION search_documents_contribution_trigger();

-- Backfill what exists.
SELECT search_documents_refresh('artist', id) FROM artists;
SELECT search_documents_refresh('album', id) FROM albums;
SELECT search_documents_refresh('composition', id) FROM compositions;
SELECT search_documents_refresh('recording', id) FROM recordings;

-- ---- ranking weights, tunable without a deploy (read on every search) ----
INSERT INTO settings (key, value) VALUES ('search_ranking', '{
  "class_weights": {"exact": 100, "normalized": 90, "prefix": 70, "fuzzy": 40},
  "origin_multipliers": {"name": 1.0, "localization": 1.0, "transliteration": 0.95, "alias": 0.9},
  "type_bonus": {"composition": 4, "recording": 3, "album": 2, "artist": 1},
  "popularity_weight": 2.0,
  "popularity_cap": 10.0,
  "language_boost": 5.0,
  "min_suggest_chars": 2,
  "cache_max_age_secs": 30
}');

-- ---- aggregate usage metrics: no queries, no users, no IPs ----
CREATE TABLE search_metrics (
    day        date     NOT NULL,
    endpoint   text     NOT NULL CHECK (endpoint IN ('search', 'suggest')),
    shard      smallint NOT NULL CHECK (shard BETWEEN 0 AND 7),   -- spreads hot-row contention
    requests   bigint   NOT NULL DEFAULT 0,
    empty      bigint   NOT NULL DEFAULT 0,
    latency_ms bigint   NOT NULL DEFAULT 0,
    accepted   bigint   NOT NULL DEFAULT 0,
    PRIMARY KEY (day, endpoint, shard)
);
