-- Views, and the hourly "fixed lists" shown on the home page (popular and new songs, albums, artists).

-- A view is counted once per viewer, target and day. Only a one-way hash of (day, viewer, target) is
-- kept, and only for two days: enough to stop reloads from inflating counts, not enough to follow anyone.
CREATE TABLE view_seen (
    seen_hash text PRIMARY KEY CHECK (length(seen_hash) = 64),
    day       date NOT NULL
);
CREATE INDEX view_seen_day_idx ON view_seen (day);

CREATE TABLE view_counts (
    kind      text NOT NULL CHECK (kind IN ('song', 'artist', 'album')),
    entity_id uuid NOT NULL,
    day       date NOT NULL,
    views     integer NOT NULL CHECK (views > 0),
    PRIMARY KEY (kind, entity_id, day)
);
CREATE INDEX view_counts_day_idx ON view_counts (kind, day);

-- Rebuilt as a whole by the worker (and once on demand on a fresh install), so a list never changes
-- between runs. `entity_id` is a composition, album or artist depending on the list.
CREATE TABLE home_lists (
    list         text NOT NULL CHECK (list IN ('popular_songs', 'new_songs', 'popular_albums', 'new_albums', 'popular_artists')),
    rank         smallint NOT NULL CHECK (rank > 0),
    entity_id    uuid NOT NULL,
    score        double precision NOT NULL,
    views        bigint NOT NULL DEFAULT 0,
    rating_avg   double precision NULL,
    rating_count integer NOT NULL DEFAULT 0,
    comments     integer NOT NULL DEFAULT 0,
    PRIMARY KEY (list, rank)
);

CREATE TABLE home_list_runs (
    id          smallint PRIMARY KEY CHECK (id = 1),
    computed_at timestamptz NOT NULL
);
