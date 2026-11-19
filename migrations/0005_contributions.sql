-- Lyric contributions and their immutable revision history (Phase 4).
--
-- A slot is (recording, type, language, script). Several contributions may share a slot
-- (alternatives, never merged); at most one of them is the primary, enforced by a partial
-- unique index and, in the application, by an advisory lock on the slot.

CREATE TABLE settings (
    key        text PRIMARY KEY CHECK (length(key) BETWEEN 1 AND 100),
    value      jsonb NOT NULL,
    updated_by uuid NULL REFERENCES users (id),
    updated_at timestamptz NOT NULL DEFAULT now()
);
-- Trust policy: who skips the review queue. Changed through the admin API, audited.
INSERT INTO settings (key, value) VALUES
    ('contribution_trust', '{"auto_publish_roles": ["moderator", "admin"], "auto_publish_min_published": null}');

CREATE TABLE contributions (
    id                    uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    recording_id          uuid NOT NULL REFERENCES recordings (id),
    type                  text NOT NULL CHECK (type IN ('original', 'translation', 'transliteration')),
    language_id           uuid NOT NULL REFERENCES languages (id),
    script_id             uuid NOT NULL REFERENCES scripts (id),
    is_primary            boolean NOT NULL DEFAULT false,
    -- State of the head revision: pending -> approved -> published, or rejected.
    status                text NOT NULL CHECK (status IN ('pending', 'approved', 'published', 'rejected')),
    current_revision_id   uuid NULL,   -- head: newest revision, whatever its state
    published_revision_id uuid NULL,   -- what the public sees; only ever moves forward on publish
    source_contribution_id uuid NULL REFERENCES contributions (id),
    review_note           text NULL CHECK (length(review_note) BETWEEN 1 AND 1000),
    created_by            uuid NOT NULL REFERENCES users (id),
    created_at            timestamptz NOT NULL DEFAULT now(),
    updated_at            timestamptz NOT NULL DEFAULT now(),
    -- A primary is always public; a published head is the public revision.
    CONSTRAINT contributions_primary_published_check CHECK (NOT is_primary OR published_revision_id IS NOT NULL),
    CONSTRAINT contributions_published_check CHECK (status <> 'published' OR published_revision_id IS NOT NULL),
    CONSTRAINT contributions_source_check CHECK (source_contribution_id IS DISTINCT FROM id)
);
-- Exactly one primary per slot.
CREATE UNIQUE INDEX contributions_primary_key ON contributions (recording_id, type, language_id, script_id)
    WHERE is_primary;
CREATE INDEX contributions_slot_idx ON contributions (recording_id, type, language_id, script_id);
CREATE INDEX contributions_recording_idx ON contributions (recording_id, created_at DESC, id DESC)
    WHERE published_revision_id IS NOT NULL;
CREATE INDEX contributions_creator_idx ON contributions (created_by, created_at DESC, id DESC);
-- Review queue: heads awaiting a decision.
CREATE INDEX contributions_queue_idx ON contributions (updated_at, id) WHERE status IN ('pending', 'approved');

CREATE TABLE lyric_revisions (
    id                      uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id         uuid NOT NULL REFERENCES contributions (id),
    revision_no             integer NOT NULL CHECK (revision_no > 0),
    parent_revision_id      uuid NULL REFERENCES lyric_revisions (id),
    body                    text NOT NULL CHECK (length(body) BETWEEN 1 AND 100000),
    note                    text NULL CHECK (length(note) BETWEEN 1 AND 500),
    restored_from_revision_id uuid NULL REFERENCES lyric_revisions (id),
    created_by              uuid NOT NULL REFERENCES users (id),
    created_at              timestamptz NOT NULL DEFAULT now(),
    UNIQUE (contribution_id, revision_no),
    -- One child per parent: history is linear even if two writers race.
    UNIQUE NULLS NOT DISTINCT (contribution_id, parent_revision_id)
);
ALTER TABLE contributions
    ADD CONSTRAINT contributions_current_revision_fk FOREIGN KEY (current_revision_id) REFERENCES lyric_revisions (id),
    ADD CONSTRAINT contributions_published_revision_fk FOREIGN KEY (published_revision_id) REFERENCES lyric_revisions (id);

-- Append-only, for every role including the table owner.
CREATE FUNCTION lyrics_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION '% is append-only', TG_TABLE_NAME USING ERRCODE = 'insufficient_privilege';
END $$;
CREATE TRIGGER lyric_revisions_no_update_delete BEFORE UPDATE OR DELETE ON lyric_revisions
    FOR EACH ROW EXECUTE FUNCTION lyrics_append_only();
CREATE TRIGGER lyric_revisions_no_truncate BEFORE TRUNCATE ON lyric_revisions
    FOR EACH STATEMENT EXECUTE FUNCTION lyrics_append_only();

-- Which revisions were ever public. The public history shows exactly these, so an
-- intermediate unpublished draft never leaks through an ancestor link.
CREATE TABLE publication_events (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id uuid NOT NULL REFERENCES contributions (id),
    revision_id     uuid NOT NULL REFERENCES lyric_revisions (id),
    actor_id        uuid NOT NULL REFERENCES users (id),
    became_primary  boolean NOT NULL DEFAULT false,
    created_at      timestamptz NOT NULL DEFAULT now(),
    UNIQUE (revision_id)
);
CREATE INDEX publication_events_contribution_idx ON publication_events (contribution_id, created_at DESC, id DESC);
CREATE TRIGGER publication_events_no_update_delete BEFORE UPDATE OR DELETE ON publication_events
    FOR EACH ROW EXECUTE FUNCTION lyrics_append_only();
CREATE TRIGGER publication_events_no_truncate BEFORE TRUNCATE ON publication_events
    FOR EACH STATEMENT EXECUTE FUNCTION lyrics_append_only();

-- The single read model for anything public (API, and later search and caches).
CREATE VIEW published_lyrics AS
SELECT c.id AS contribution_id, c.recording_id, c.type, c.language_id, c.script_id, c.is_primary,
       r.id AS revision_id, r.body, r.created_at AS revised_at
FROM contributions c
JOIN lyric_revisions r ON r.id = c.published_revision_id;

-- Retryable writes. The key row is written in the same transaction as the work, so a failed
-- attempt leaves nothing behind and a concurrent retry waits on the unique index.
CREATE TABLE idempotency_keys (
    user_id       uuid NOT NULL REFERENCES users (id),
    key           text NOT NULL CHECK (length(key) BETWEEN 8 AND 128),
    fingerprint   text NOT NULL,
    result_kind   text NULL,
    result_id     uuid NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, key)
);
CREATE INDEX idempotency_keys_age_idx ON idempotency_keys (created_at);
