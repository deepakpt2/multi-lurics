-- Songs a moderator has chosen to feature on the home page. Curated, so it changes at once
-- (unlike the computed lists); a song that loses its public lyrics drops out when read.
CREATE TABLE editors_picks (
    composition_id uuid PRIMARY KEY REFERENCES compositions (id) ON DELETE CASCADE,
    picked_by      uuid NOT NULL REFERENCES users (id),
    picked_at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX editors_picks_recent_idx ON editors_picks (picked_at DESC, composition_id);
