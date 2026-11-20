-- Indian song metadata. Title, year, lyricist (credit), music director (composer credit), singers
-- (performer credits on the recording) and album (album_tracks) already exist and are reused, not duplicated.
-- A song is mainstream (film, album, indie) or devotional; only devotional songs carry religion and deity.
ALTER TABLE compositions
    ADD COLUMN category text   NOT NULL DEFAULT 'mainstream' CHECK (category IN ('mainstream', 'devotional')),
    ADD COLUMN raga     text   NULL CHECK (length(raga) BETWEEN 1 AND 100),
    ADD COLUMN religion text   NULL CHECK (religion IN ('hindu', 'sikh', 'buddhist', 'jain', 'christian', 'muslim', 'other')),
    ADD COLUMN deity    text   NULL CHECK (length(deity) BETWEEN 1 AND 100),
    ADD COLUMN tags     text[] NOT NULL DEFAULT '{}' CHECK (cardinality(tags) <= 20),
    ADD CONSTRAINT compositions_devotional_check CHECK (category = 'devotional' OR (religion IS NULL AND deity IS NULL));

CREATE INDEX compositions_category_idx ON compositions (category, created_at DESC, id DESC);
CREATE INDEX compositions_tags_idx ON compositions USING gin (tags);
