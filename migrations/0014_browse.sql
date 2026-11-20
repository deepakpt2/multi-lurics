-- Browse (Phase 10): readable, id-free URLs for the things a visitor filters songs by.
--
-- A browse filter is addressed by a slug in the entity's primary language: `?singer=yesudas&album=jeevitha-naukha`.
-- Artists and albums get exactly one slug each, made from the canonical stored name, never from an id.
-- Like page slugs, a retired slug is kept for good and still resolves (merged entities follow `entity_merges`).
-- Slugs are made by `infrastructure::browse::sync_slug` (worker on change, and on demand when a list needs one).

CREATE TABLE browse_slugs (
    entity_type text        NOT NULL CHECK (entity_type IN ('artist', 'album')),
    entity_id   uuid        NOT NULL,
    slug        text        NOT NULL CHECK (length(slug) BETWEEN 1 AND 200 AND slug = normalize(slug, NFC)),
    is_current  boolean     NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (entity_type, slug)
);
CREATE UNIQUE INDEX browse_slugs_current_key ON browse_slugs (entity_type, entity_id) WHERE is_current;
CREATE INDEX browse_slugs_entity_idx ON browse_slugs (entity_type, entity_id);

-- Filters by person and role, and the "songs of this singer" joins.
CREATE INDEX credits_artist_role_idx ON credits (artist_id, role);
CREATE INDEX credits_recording_artist_idx ON credits (recording_id, artist_id) WHERE recording_id IS NOT NULL;
CREATE INDEX credits_composition_artist_idx ON credits (composition_id, artist_id) WHERE composition_id IS NOT NULL;

-- Raga and deity are free text; their URL form is the normalized key with dashes for spaces.
CREATE INDEX compositions_raga_key_idx ON compositions (replace(lyrics_search_key(raga), ' ', '-')) WHERE raga IS NOT NULL;
CREATE INDEX compositions_deity_key_idx ON compositions (replace(lyrics_search_key(deity), ' ', '-')) WHERE deity IS NOT NULL;
CREATE INDEX compositions_religion_idx ON compositions (religion) WHERE religion IS NOT NULL;

-- "Which languages has this song's published lyrics in".
CREATE INDEX contributions_published_language_idx ON contributions (recording_id, language_id)
    WHERE published_revision_id IS NOT NULL;
