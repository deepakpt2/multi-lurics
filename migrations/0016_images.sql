-- Images for entities (artists, albums, ragas, deities, ...). Files live in the media store
-- (local disk today); this table is the record of who uploaded what, its review state, and which
-- image is the public one. A new image never replaces the public one until a moderator (or the
-- trust policy that already governs lyrics) approves it.
CREATE TABLE entity_images (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_type      text NULL CHECK (entity_type IN ('artist','album','label','publisher','genre','raga','deity')),
    entity_id        uuid NULL,
    proposal_id      uuid NULL REFERENCES entity_proposals (id),
    -- draft: uploaded, not yet attached to anything. pending: waiting for review.
    status           text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','pending','approved','rejected','superseded')),
    width            integer NOT NULL CHECK (width > 0),
    height           integer NOT NULL CHECK (height > 0),
    bytes            integer NOT NULL CHECK (bytes > 0),
    sizes            integer[] NOT NULL CHECK (cardinality(sizes) > 0),
    sha256           text NOT NULL CHECK (length(sha256) = 64),
    credit           text NULL CHECK (length(credit) BETWEEN 1 AND 300),
    rights_confirmed boolean NOT NULL CHECK (rights_confirmed),
    created_by       uuid NOT NULL REFERENCES users (id),
    created_at       timestamptz NOT NULL DEFAULT now(),
    decided_by       uuid NULL REFERENCES users (id),
    decided_at       timestamptz NULL,
    decision_note    text NULL CHECK (length(decision_note) BETWEEN 1 AND 1000),
    CONSTRAINT entity_images_target_check CHECK ((entity_type IS NULL) = (entity_id IS NULL)),
    CONSTRAINT entity_images_attached_check CHECK ((status = 'draft') = (entity_id IS NULL AND proposal_id IS NULL))
);
-- One public image per entity, even when two approvals race.
CREATE UNIQUE INDEX entity_images_current_key ON entity_images (entity_type, entity_id) WHERE status = 'approved';
-- A person has at most one image waiting per entity.
CREATE UNIQUE INDEX entity_images_pending_key ON entity_images (entity_type, entity_id, created_by)
    WHERE status = 'pending' AND entity_id IS NOT NULL;
CREATE INDEX entity_images_queue_idx ON entity_images (created_at, id) WHERE status = 'pending';
CREATE INDEX entity_images_owner_idx ON entity_images (created_by, status);
CREATE INDEX entity_images_proposal_idx ON entity_images (proposal_id) WHERE proposal_id IS NOT NULL;
