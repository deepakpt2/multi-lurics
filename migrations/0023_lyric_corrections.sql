-- A reader's suggested fix to a published lyric. It waits in the moderators' queue; applying it makes
-- a new revision of the lyric and publishes it, so history and the audit log work as for any edit.
CREATE TABLE lyric_corrections (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    contribution_id  uuid NOT NULL REFERENCES contributions (id) ON DELETE CASCADE,
    body             text NOT NULL,
    note             text,
    status           text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'applied', 'rejected')),
    created_by       uuid NOT NULL REFERENCES users (id),
    created_at       timestamptz NOT NULL DEFAULT now(),
    decided_by       uuid REFERENCES users (id),
    decided_at       timestamptz,
    decision_note    text
);
-- One open suggestion per person per lyric; they can send another after it is decided.
CREATE UNIQUE INDEX lyric_corrections_open ON lyric_corrections (contribution_id, created_by) WHERE status = 'pending';
CREATE INDEX lyric_corrections_queue ON lyric_corrections (created_at, id) WHERE status = 'pending';
