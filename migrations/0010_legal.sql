-- Legal workflows: notices from anyone (no account needed), takedown decided by a moderator,
-- appeal by the contributor decided by an admin. Revisions are never touched: a takedown only
-- withdraws the public pointer, so a reinstatement restores exactly what was published.

CREATE TABLE legal_notices (
    id                     uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    reference              text NOT NULL UNIQUE,
    kind                   text NOT NULL CHECK (kind IN ('copyright', 'privacy', 'content')),
    contribution_id        uuid REFERENCES contributions (id),
    contact_name           text NOT NULL CHECK (char_length(contact_name) BETWEEN 1 AND 200),
    contact_email          text NOT NULL CHECK (char_length(contact_email) BETWEEN 3 AND 254),
    statement              text NOT NULL CHECK (char_length(statement) BETWEEN 20 AND 5000),
    good_faith             boolean NOT NULL CHECK (good_faith),
    status                 text NOT NULL DEFAULT 'open'
                           CHECK (status IN ('open', 'taken_down', 'rejected', 'closed', 'reinstated')),
    decided_by             uuid REFERENCES users (id),
    decided_at             timestamptz,
    decision_note          text CHECK (char_length(decision_note) <= 1000),
    taken_down_revision_id uuid REFERENCES lyric_revisions (id),
    was_primary            boolean NOT NULL DEFAULT false,
    created_at             timestamptz NOT NULL DEFAULT now(),
    -- A takedown or a copyright/content notice always names what it is about.
    CONSTRAINT legal_notices_target_check CHECK (kind = 'privacy' OR contribution_id IS NOT NULL)
);
CREATE INDEX legal_notices_queue ON legal_notices (status, created_at, id);

CREATE TABLE takedown_appeals (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    notice_id     uuid NOT NULL REFERENCES legal_notices (id),
    appellant_id  uuid NOT NULL REFERENCES users (id),
    statement     text NOT NULL CHECK (char_length(statement) BETWEEN 20 AND 5000),
    status        text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'upheld', 'denied')),
    decided_by    uuid REFERENCES users (id),
    decision_note text CHECK (char_length(decision_note) <= 1000),
    decided_at    timestamptz,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX takedown_appeals_open_key ON takedown_appeals (notice_id) WHERE status = 'open';
CREATE INDEX takedown_appeals_queue ON takedown_appeals (status, created_at, id);
