-- Comparison, community and trust (Phase 8).
--
-- Everything here points at contributions by id and never copies lyric text. Public reads
-- re-check `published_revision_id` live, so unpublishing hides community content at once.
-- Reputation is an append-only event log; it is NOT a role (roles stay in user_roles).

-- ---- saved comparisons: ids only ----
CREATE TABLE saved_comparisons (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id         uuid NOT NULL REFERENCES users (id),
    name             text NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    contribution_ids uuid[] NOT NULL CHECK (cardinality(contribution_ids) BETWEEN 1 AND 4),
    created_at       timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX saved_comparisons_owner_idx ON saved_comparisons (owner_id, created_at DESC, id DESC);

-- ---- ratings, reactions, bookmarks ----
CREATE TABLE ratings (
    contribution_id uuid NOT NULL REFERENCES contributions (id),
    user_id         uuid NOT NULL REFERENCES users (id),
    value           smallint NOT NULL CHECK (value BETWEEN 1 AND 5),
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (contribution_id, user_id)
);
-- No self-rating, backed up in the database: neither the contribution's creator nor anyone who
-- wrote one of its revisions may rate it.
CREATE FUNCTION ratings_no_self() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM contributions WHERE id = NEW.contribution_id AND created_by = NEW.user_id)
       OR EXISTS (SELECT 1 FROM lyric_revisions WHERE contribution_id = NEW.contribution_id AND created_by = NEW.user_id) THEN
        RAISE EXCEPTION 'self rating' USING ERRCODE = 'check_violation', CONSTRAINT = 'ratings_no_self';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER ratings_no_self BEFORE INSERT OR UPDATE ON ratings FOR EACH ROW EXECUTE FUNCTION ratings_no_self();

CREATE TABLE reactions (
    contribution_id uuid NOT NULL REFERENCES contributions (id),
    user_id         uuid NOT NULL REFERENCES users (id),
    kind            text NOT NULL CHECK (kind IN ('like', 'moving', 'insightful')),
    created_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (contribution_id, user_id, kind)
);

CREATE TABLE bookmarks (
    user_id         uuid NOT NULL REFERENCES users (id),
    contribution_id uuid NOT NULL REFERENCES contributions (id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, contribution_id)
);
CREATE INDEX bookmarks_user_idx ON bookmarks (user_id, created_at DESC, contribution_id DESC);

-- ---- comments ----
CREATE TABLE comments (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id uuid NOT NULL REFERENCES contributions (id),
    author_id       uuid NOT NULL REFERENCES users (id),
    body            text NOT NULL CHECK (length(body) BETWEEN 1 AND 5000),
    status          text NOT NULL DEFAULT 'visible' CHECK (status IN ('visible', 'hidden', 'removed')),
    hidden_by       uuid NULL REFERENCES users (id),   -- the moderator who hid it (appeals go to someone else)
    version         integer NOT NULL DEFAULT 1,
    created_at      timestamptz NOT NULL DEFAULT now(),
    edited_at       timestamptz NULL
);
CREATE INDEX comments_contribution_idx ON comments (contribution_id, created_at, id) WHERE status = 'visible';
CREATE INDEX comments_author_idx ON comments (author_id, created_at DESC);

-- ---- profiles ----
CREATE TABLE user_profiles (
    user_id    uuid PRIMARY KEY REFERENCES users (id),
    bio        text NOT NULL DEFAULT '' CHECK (length(bio) <= 1000),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- ---- reputation: an append-only event log. The score is the sum; it never changes a role. ----
CREATE TABLE reputation_events (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      uuid NOT NULL REFERENCES users (id),
    kind         text NOT NULL CHECK (kind ~ '^[a-z_]+$'),
    points       integer NOT NULL,
    subject_type text NOT NULL,
    subject_id   uuid NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    -- A redelivered or repeated cause scores once.
    UNIQUE (user_id, kind, subject_id)
);
CREATE INDEX reputation_events_user_idx ON reputation_events (user_id, created_at DESC, id DESC);
CREATE TRIGGER reputation_events_no_update_delete BEFORE UPDATE OR DELETE ON reputation_events
    FOR EACH ROW EXECUTE FUNCTION lyrics_append_only();
CREATE TRIGGER reputation_events_no_truncate BEFORE TRUNCATE ON reputation_events
    FOR EACH STATEMENT EXECUTE FUNCTION lyrics_append_only();

-- ---- reports, moderator notes, appeals ----
CREATE TABLE reports (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    reporter_id     uuid NOT NULL REFERENCES users (id),
    target_type     text NOT NULL CHECK (target_type IN ('contribution', 'comment')),
    target_id       uuid NOT NULL,
    reason          text NOT NULL CHECK (reason IN ('spam', 'abuse', 'copyright', 'inaccurate', 'other')),
    detail          text NULL CHECK (length(detail) BETWEEN 1 AND 2000),
    status          text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'upheld', 'dismissed')),
    resolved_by     uuid NULL REFERENCES users (id),
    resolution_note text NULL CHECK (length(resolution_note) BETWEEN 1 AND 1000),
    resolved_at     timestamptz NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    CHECK ((status = 'open') = (resolved_at IS NULL))
);
-- One open report per reporter and target: repeats are a conflict, not noise.
CREATE UNIQUE INDEX reports_open_key ON reports (reporter_id, target_type, target_id) WHERE status = 'open';
CREATE INDEX reports_queue_idx ON reports (created_at, id) WHERE status = 'open';

CREATE TABLE moderator_notes (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    subject_type text NOT NULL CHECK (subject_type IN ('contribution', 'comment', 'user', 'report')),
    subject_id   uuid NOT NULL,
    author_id    uuid NOT NULL REFERENCES users (id),
    body         text NOT NULL CHECK (length(body) BETWEEN 1 AND 2000),
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX moderator_notes_subject_idx ON moderator_notes (subject_type, subject_id, created_at DESC, id DESC);
CREATE TRIGGER moderator_notes_no_update_delete BEFORE UPDATE OR DELETE ON moderator_notes
    FOR EACH ROW EXECUTE FUNCTION lyrics_append_only();

CREATE TABLE appeals (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    appellant_id  uuid NOT NULL REFERENCES users (id),
    target_type   text NOT NULL CHECK (target_type IN ('comment')),
    target_id     uuid NOT NULL,
    message       text NOT NULL CHECK (length(message) BETWEEN 1 AND 2000),
    status        text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'upheld', 'denied')),
    decided_by    uuid NULL REFERENCES users (id),
    decision_note text NULL CHECK (length(decision_note) BETWEEN 1 AND 1000),
    decided_at    timestamptz NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    CHECK ((status = 'open') = (decided_at IS NULL))
);
CREATE UNIQUE INDEX appeals_open_key ON appeals (target_type, target_id) WHERE status = 'open';
CREATE INDEX appeals_queue_idx ON appeals (created_at, id) WHERE status = 'open';

-- ---- abuse controls ----
-- One row per rate-limited action; the window is counted in SQL so every API replica agrees.
CREATE TABLE abuse_events (
    user_id uuid NOT NULL REFERENCES users (id),
    action  text NOT NULL,
    at      timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX abuse_events_idx ON abuse_events (user_id, action, at DESC);

-- Tunable without a deploy (admin API, audited). Thresholds gate community actions on global
-- reputation; they are not roles.
INSERT INTO settings (key, value) VALUES ('community', '{
  "points": {"contribution_published": 10, "rating_received_high": 1, "comment_removed": -3, "report_upheld_against": -5, "report_upheld": 2},
  "min_reputation": {"rate": 0, "react": 0, "comment": 0, "report": 0, "appeal": 0},
  "levels": [{"name": "newcomer", "min": 0}, {"name": "regular", "min": 20}, {"name": "trusted", "min": 100}],
  "limits": {"comment_chars": 2000, "bio_chars": 500, "report_detail_chars": 1000, "appeal_chars": 1000, "note_chars": 1000},
  "rate": {
    "comment": {"max": 10, "window_secs": 600},
    "rating": {"max": 60, "window_secs": 600},
    "reaction": {"max": 120, "window_secs": 600},
    "bookmark": {"max": 120, "window_secs": 600},
    "report": {"max": 10, "window_secs": 3600},
    "appeal": {"max": 5, "window_secs": 3600},
    "comparison": {"max": 30, "window_secs": 3600}
  },
  "spam": {"max_links": 2, "max_repeat_run": 12, "duplicate_window_secs": 600, "blocked_terms": []}
}');
