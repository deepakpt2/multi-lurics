-- Core catalogue: artists, compositions, recordings, albums, credits, recording relationships.
-- Strong identifiers are stored normalized (format enforced by CHECK) and unique, so the
-- duplicate guard holds even when the application check races. Names/titles are never unique.

CREATE TABLE artists (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    name            text        NOT NULL CHECK (length(name) BETWEEN 1 AND 300),
    kind            text        NOT NULL CHECK (kind IN ('person', 'group', 'other')),
    disambiguation  text        NULL CHECK (length(disambiguation) BETWEEN 1 AND 200),
    isni            text        NULL CHECK (isni ~ '^[0-9]{15}[0-9X]$'),
    version         integer     NOT NULL DEFAULT 1 CHECK (version > 0),
    created_by      uuid        NOT NULL REFERENCES users (id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT artists_isni_key UNIQUE (isni)
);
CREATE INDEX artists_list_idx ON artists (created_at DESC, id DESC);
CREATE INDEX artists_name_prefix_idx ON artists (lower(name) text_pattern_ops);

CREATE TABLE compositions (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    title       text        NOT NULL CHECK (length(title) BETWEEN 1 AND 300),
    iswc        text        NULL CHECK (iswc ~ '^T[0-9]{10}$'),
    year        smallint    NULL CHECK (year BETWEEN 1000 AND 2100),
    version     integer     NOT NULL DEFAULT 1 CHECK (version > 0),
    created_by  uuid        NOT NULL REFERENCES users (id),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT compositions_iswc_key UNIQUE (iswc)
);
CREATE INDEX compositions_list_idx ON compositions (created_at DESC, id DESC);
CREATE INDEX compositions_title_prefix_idx ON compositions (lower(title) text_pattern_ops);

CREATE TABLE recordings (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    composition_id  uuid        NOT NULL REFERENCES compositions (id),
    title           text        NOT NULL CHECK (length(title) BETWEEN 1 AND 300),
    isrc            text        NULL CHECK (isrc ~ '^[A-Z]{2}[A-Z0-9]{3}[0-9]{7}$'),
    duration_ms     integer     NULL CHECK (duration_ms > 0 AND duration_ms < 86400000),
    year            smallint    NULL CHECK (year BETWEEN 1000 AND 2100),
    version         integer     NOT NULL DEFAULT 1 CHECK (version > 0),
    created_by      uuid        NOT NULL REFERENCES users (id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT recordings_isrc_key UNIQUE (isrc),
    -- Target of the composite FKs below: pins a relationship to its shared composition.
    CONSTRAINT recordings_id_composition_key UNIQUE (id, composition_id)
);
CREATE INDEX recordings_list_idx ON recordings (created_at DESC, id DESC);
CREATE INDEX recordings_composition_idx ON recordings (composition_id, created_at DESC, id DESC);
CREATE INDEX recordings_title_prefix_idx ON recordings (lower(title) text_pattern_ops);

CREATE TABLE albums (
    id            uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    title         text        NOT NULL CHECK (length(title) BETWEEN 1 AND 300),
    kind          text        NOT NULL CHECK (kind IN ('album', 'single', 'ep', 'compilation', 'soundtrack', 'other')),
    release_year  smallint    NULL CHECK (release_year BETWEEN 1000 AND 2100),
    barcode       text        NULL CHECK (barcode ~ '^([0-9]{8}|[0-9]{12,14})$'),
    version       integer     NOT NULL DEFAULT 1 CHECK (version > 0),
    created_by    uuid        NOT NULL REFERENCES users (id),
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT albums_barcode_key UNIQUE (barcode)
);
CREATE INDEX albums_list_idx ON albums (created_at DESC, id DESC);
CREATE INDEX albums_title_prefix_idx ON albums (lower(title) text_pattern_ops);

CREATE TABLE album_tracks (
    album_id      uuid    NOT NULL REFERENCES albums (id),
    position      integer NOT NULL CHECK (position BETWEEN 1 AND 999),
    recording_id  uuid    NOT NULL REFERENCES recordings (id),
    PRIMARY KEY (album_id, position),
    CONSTRAINT album_tracks_recording_key UNIQUE (album_id, recording_id)
);
CREATE INDEX album_tracks_recording_idx ON album_tracks (recording_id);

-- One credit = one artist in one role on exactly one entity. No free-text author field exists.
CREATE TABLE credits (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    artist_id       uuid        NOT NULL REFERENCES artists (id),
    role            text        NOT NULL,
    composition_id  uuid        NULL REFERENCES compositions (id),
    recording_id    uuid        NULL REFERENCES recordings (id),
    album_id        uuid        NULL REFERENCES albums (id),
    position        integer     NOT NULL CHECK (position >= 0),
    created_at      timestamptz NOT NULL DEFAULT now(),
    CHECK (num_nonnulls(composition_id, recording_id, album_id) = 1),
    CHECK (
        (composition_id IS NOT NULL AND role IN ('composer', 'lyricist'))
        OR (recording_id IS NOT NULL AND role IN ('performer', 'featured_performer', 'arranger', 'producer', 'conductor'))
        OR (album_id IS NOT NULL AND role IN ('primary_artist', 'producer'))
    )
);
CREATE UNIQUE INDEX credits_composition_key ON credits (composition_id, artist_id, role) WHERE composition_id IS NOT NULL;
CREATE UNIQUE INDEX credits_recording_key ON credits (recording_id, artist_id, role) WHERE recording_id IS NOT NULL;
CREATE UNIQUE INDEX credits_album_key ON credits (album_id, artist_id, role) WHERE album_id IS NOT NULL;
CREATE INDEX credits_artist_idx ON credits (artist_id);

-- `recording_id` is the derived recording (the cover, the live take); `related_recording_id` is
-- the one it relates to. Every kind except related_performance is a recording of the same work:
-- `composition_id` is then set and the composite FKs make a cross-composition link impossible,
-- including under concurrent edits. Relationships carry metadata only, never lyrics.
CREATE TABLE recording_relationships (
    id                    uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    kind                  text        NOT NULL
                                      CHECK (kind IN ('cover', 'remake', 'live', 'alternate_arrangement', 'related_performance')),
    recording_id          uuid        NOT NULL REFERENCES recordings (id),
    related_recording_id  uuid        NOT NULL REFERENCES recordings (id),
    composition_id        uuid        NULL,
    created_by            uuid        NOT NULL REFERENCES users (id),
    created_at            timestamptz NOT NULL DEFAULT now(),
    CHECK (recording_id <> related_recording_id),
    CHECK ((kind = 'related_performance') = (composition_id IS NULL)),
    CONSTRAINT recording_relationships_key UNIQUE (recording_id, related_recording_id, kind),
    CONSTRAINT recording_relationships_from_fk FOREIGN KEY (recording_id, composition_id)
        REFERENCES recordings (id, composition_id),
    CONSTRAINT recording_relationships_to_fk FOREIGN KEY (related_recording_id, composition_id)
        REFERENCES recordings (id, composition_id)
);
CREATE INDEX recording_relationships_related_idx ON recording_relationships (related_recording_id);
