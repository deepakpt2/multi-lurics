-- Phase 5: one normalized search index for every entity picker, proposals, and merge history.
--
-- `search_keys` is derived data: every canonical name, verified localization and verified alias of
-- an entity becomes one normalized key. Triggers keep it in step, so no write path can forget.
-- Display values are never touched; only the key is normalized. Pending or rejected names are
-- never indexed.

CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Lowercase, compatibility-decomposed, with Latin/Arabic diacritics, tatweel, joiners and bidi
-- marks removed, punctuation turned into spaces, whitespace collapsed. Indic vowel signs and
-- viramas are kept: they change the word.
CREATE FUNCTION lyrics_search_key(s text) RETURNS text LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE AS $$
    SELECT btrim(regexp_replace(
        regexp_replace(
            regexp_replace(lower(normalize(s, NFKD)),
                '[̀-ًͯ-ْـ​-‏‪-‮⁦-⁩﻿]', '', 'g'),
            '[!-/:-@\[-`{-~¡-¿‐-‧‰-⁞、-〃،؛؟۔।॥]', ' ', 'g'),
        '\s+', ' ', 'g'))
$$;

CREATE TABLE search_keys (
    entity_type text NOT NULL CHECK (entity_type IN ('artist','album','label','publisher','genre','language','script')),
    entity_id   uuid NOT NULL,
    origin      text NOT NULL CHECK (origin IN ('name','localization','alias')),
    key         text NOT NULL CHECK (key <> ''),
    compact     text NOT NULL CHECK (compact <> ''),
    PRIMARY KEY (entity_type, entity_id, key)
);
CREATE INDEX search_keys_trgm_idx ON search_keys USING gin (key gin_trgm_ops);
CREATE INDEX search_keys_compact_idx ON search_keys (entity_type, compact);

CREATE FUNCTION search_keys_refresh(t text, eid uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE canonical text[];
BEGIN
    canonical := CASE t
        WHEN 'artist'    THEN ARRAY(SELECT name FROM artists WHERE id = eid)
        WHEN 'album'     THEN ARRAY(SELECT title FROM albums WHERE id = eid)
        WHEN 'label'     THEN ARRAY(SELECT name FROM labels WHERE id = eid)
        WHEN 'publisher' THEN ARRAY(SELECT name FROM publishers WHERE id = eid)
        WHEN 'genre'     THEN ARRAY(SELECT name FROM genres WHERE id = eid)
        WHEN 'language'  THEN ARRAY(SELECT x FROM languages l, unnest(ARRAY[l.name_en, l.endonym, l.iso639_3, l.iso639_1]) x WHERE l.id = eid)
        WHEN 'script'    THEN ARRAY(SELECT x FROM scripts s, unnest(ARRAY[s.name_en, s.iso15924]) x WHERE s.id = eid)
    END;
    DELETE FROM search_keys WHERE entity_type = t AND entity_id = eid;
    INSERT INTO search_keys (entity_type, entity_id, origin, key, compact)
    SELECT t, eid, o, k, replace(k, ' ', '')
    FROM (
        SELECT 1 AS prio, 'name' AS o, lyrics_search_key(v) AS k FROM unnest(canonical) v WHERE v IS NOT NULL
        UNION ALL
        SELECT 2, 'localization', lyrics_search_key(name) FROM entity_localizations
            WHERE entity_type = t AND entity_id = eid AND verification_state = 'verified'
        UNION ALL
        SELECT 3, 'alias', lyrics_search_key(alias) FROM entity_aliases
            WHERE entity_type = t AND entity_id = eid AND verification_state = 'verified'
    ) s
    WHERE k <> ''
    ORDER BY prio
    ON CONFLICT DO NOTHING;
END $$;

CREATE FUNCTION search_keys_entity_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        DELETE FROM search_keys WHERE entity_type = TG_ARGV[0] AND entity_id = OLD.id;
        RETURN NULL;
    END IF;
    PERFORM search_keys_refresh(TG_ARGV[0], NEW.id);
    RETURN NULL;
END $$;

CREATE FUNCTION search_keys_child_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'DELETE' AND NEW.entity_type IN ('artist','album','label','publisher','genre') THEN
        PERFORM search_keys_refresh(NEW.entity_type, NEW.entity_id);
    END IF;
    IF TG_OP <> 'INSERT' AND OLD.entity_type IN ('artist','album','label','publisher','genre')
       AND (TG_OP = 'DELETE' OR (OLD.entity_type, OLD.entity_id) IS DISTINCT FROM (NEW.entity_type, NEW.entity_id)) THEN
        PERFORM search_keys_refresh(OLD.entity_type, OLD.entity_id);
    END IF;
    RETURN NULL;
END $$;

CREATE TRIGGER artists_search AFTER INSERT OR UPDATE OF name OR DELETE ON artists
    FOR EACH ROW EXECUTE FUNCTION search_keys_entity_trigger('artist');
CREATE TRIGGER albums_search AFTER INSERT OR UPDATE OF title OR DELETE ON albums
    FOR EACH ROW EXECUTE FUNCTION search_keys_entity_trigger('album');
CREATE TRIGGER labels_search AFTER INSERT OR UPDATE OF name OR DELETE ON labels
    FOR EACH ROW EXECUTE FUNCTION search_keys_entity_trigger('label');
CREATE TRIGGER publishers_search AFTER INSERT OR UPDATE OF name OR DELETE ON publishers
    FOR EACH ROW EXECUTE FUNCTION search_keys_entity_trigger('publisher');
CREATE TRIGGER genres_search AFTER INSERT OR UPDATE OF name OR DELETE ON genres
    FOR EACH ROW EXECUTE FUNCTION search_keys_entity_trigger('genre');
CREATE TRIGGER languages_search AFTER INSERT OR UPDATE OF name_en, endonym, iso639_3, iso639_1 ON languages
    FOR EACH ROW EXECUTE FUNCTION search_keys_entity_trigger('language');
CREATE TRIGGER scripts_search AFTER INSERT OR UPDATE OF name_en, iso15924 ON scripts
    FOR EACH ROW EXECUTE FUNCTION search_keys_entity_trigger('script');
CREATE TRIGGER localizations_search AFTER INSERT OR UPDATE OR DELETE ON entity_localizations
    FOR EACH ROW EXECUTE FUNCTION search_keys_child_trigger();
CREATE TRIGGER aliases_search AFTER INSERT OR UPDATE OR DELETE ON entity_aliases
    FOR EACH ROW EXECUTE FUNCTION search_keys_child_trigger();

-- Backfill what exists.
SELECT search_keys_refresh('artist', id) FROM artists;
SELECT search_keys_refresh('album', id) FROM albums;
SELECT search_keys_refresh('label', id) FROM labels;
SELECT search_keys_refresh('publisher', id) FROM publishers;
SELECT search_keys_refresh('genre', id) FROM genres;
SELECT search_keys_refresh('language', id) FROM languages;
SELECT search_keys_refresh('script', id) FROM scripts;

-- Someone proposes a new entity; a moderator approves it (creating the canonical entity and its
-- aliases) or rejects it. `shown_matches` records the likely matches the proposer was shown.
CREATE TABLE entity_proposals (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_type     text NOT NULL CHECK (entity_type IN ('artist','album','label','publisher','genre')),
    name            text NOT NULL CHECK (length(name) BETWEEN 1 AND 300),
    compact         text NOT NULL CHECK (compact <> ''),
    kind            text NULL CHECK (length(kind) BETWEEN 1 AND 30),
    disambiguation  text NULL CHECK (length(disambiguation) BETWEEN 1 AND 200),
    aliases         text[] NOT NULL DEFAULT '{}' CHECK (cardinality(aliases) <= 10),
    note            text NULL CHECK (length(note) BETWEEN 1 AND 1000),
    shown_matches   uuid[] NOT NULL DEFAULT '{}',
    status          text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected')),
    created_by      uuid NOT NULL REFERENCES users (id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    decided_by      uuid NULL REFERENCES users (id),
    decided_at      timestamptz NULL,
    decision_note   text NULL CHECK (length(decision_note) BETWEEN 1 AND 1000),
    entity_id       uuid NULL,   -- the canonical entity an approval created
    duplicate_of_id uuid NULL,   -- the existing entity a rejection points to
    CONSTRAINT entity_proposals_decided_check CHECK (
        (status = 'pending') = (decided_by IS NULL AND decided_at IS NULL)
        AND (status = 'approved') = (entity_id IS NOT NULL))
);
-- The same name cannot be queued twice, even when two requests race.
CREATE UNIQUE INDEX entity_proposals_pending_key ON entity_proposals (entity_type, compact) WHERE status = 'pending';
CREATE INDEX entity_proposals_queue_idx ON entity_proposals (created_at, id) WHERE status = 'pending';

-- A merged entity is gone; its id keeps resolving through this table. Append-only history.
CREATE TABLE entity_merges (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_type text NOT NULL CHECK (entity_type IN ('artist','album','label','publisher','genre')),
    from_id     uuid NOT NULL,
    into_id     uuid NOT NULL,
    actor_id    uuid NOT NULL REFERENCES users (id),
    note        text NULL CHECK (length(note) BETWEEN 1 AND 1000),
    snapshot    jsonb NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT entity_merges_distinct CHECK (from_id <> into_id),
    CONSTRAINT entity_merges_from_key UNIQUE (from_id)
);
CREATE INDEX entity_merges_into_idx ON entity_merges (into_id);
CREATE TRIGGER entity_merges_no_update_delete BEFORE UPDATE OR DELETE ON entity_merges
    FOR EACH ROW EXECUTE FUNCTION lyrics_append_only();
CREATE TRIGGER entity_merges_no_truncate BEFORE TRUNCATE ON entity_merges
    FOR EACH STATEMENT EXECUTE FUNCTION lyrics_append_only();

-- A localization whose slot the surviving entity already holds is kept as retired history on the
-- survivor (state `rejected`, name preserved as an alias) instead of being silently dropped.
ALTER TABLE entity_localizations ADD COLUMN merged_from_id uuid NULL;
DROP INDEX localizations_slot_key;
CREATE UNIQUE INDEX localizations_slot_key ON entity_localizations
    (entity_type, entity_id, language_id, coalesce(script_id, '00000000-0000-0000-0000-000000000000'::uuid))
    WHERE merged_from_id IS NULL;
-- Index order decides which unique violation Postgres reports first when a write breaks both:
-- keep "slot taken" ahead of "preferred taken", as before.
DROP INDEX localizations_preferred_key;
CREATE UNIQUE INDEX localizations_preferred_key ON entity_localizations (entity_type, entity_id, language_id)
    WHERE is_preferred;
