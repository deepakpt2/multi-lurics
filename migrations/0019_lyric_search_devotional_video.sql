-- Whole-text lyric search, devotional structure, a YouTube video per song, more languages.

-- ---- Whole-text search over published lyrics ----
-- One row per published lyric. `lines` has one normalized entry per line of the body (blank lines kept,
-- so entry N is line N of the text as written); `key` is the same text joined with spaces, which is what
-- the trigram index serves. Rows follow publication: a trigger rewrites or removes them.
CREATE TABLE lyric_search (
    contribution_id uuid   PRIMARY KEY REFERENCES contributions (id) ON DELETE CASCADE,
    composition_id  uuid   NOT NULL REFERENCES compositions (id) ON DELETE CASCADE,
    language_id     uuid   NOT NULL,
    lines           text[] NOT NULL,
    key             text   NOT NULL
);
CREATE INDEX lyric_search_key_trgm ON lyric_search USING gin (key gin_trgm_ops);
CREATE INDEX lyric_search_composition_idx ON lyric_search (composition_id);

CREATE FUNCTION lyric_search_refresh(cid uuid) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    DELETE FROM lyric_search WHERE contribution_id = cid;
    INSERT INTO lyric_search (contribution_id, composition_id, language_id, lines, key)
    SELECT c.id, r.composition_id, c.language_id, l.lines, btrim(regexp_replace(array_to_string(l.lines, ' '), '\s+', ' ', 'g'))
    FROM contributions c
    JOIN recordings r ON r.id = c.recording_id
    JOIN lyric_revisions v ON v.id = c.published_revision_id
    CROSS JOIN LATERAL (
        SELECT array_agg(lyrics_search_key(x.line) ORDER BY x.n) AS lines
        FROM unnest(string_to_array(v.body, E'\n')) WITH ORDINALITY AS x(line, n)
    ) l
    WHERE c.id = cid AND l.lines IS NOT NULL;
END $$;

CREATE FUNCTION lyric_search_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' OR NEW.published_revision_id IS DISTINCT FROM OLD.published_revision_id THEN
        PERFORM lyric_search_refresh(NEW.id);
    END IF;
    RETURN NULL;
END $$;
CREATE TRIGGER contributions_lyric_search AFTER INSERT OR UPDATE OF published_revision_id ON contributions
    FOR EACH ROW EXECUTE FUNCTION lyric_search_trigger();

SELECT lyric_search_refresh(id) FROM contributions WHERE published_revision_id IS NOT NULL;

-- ---- A YouTube video per song, and devotional structure ----
-- The video is stored as its 11-character id, never as a link: the page builds the embed address, so a
-- change in YouTube's address scheme is one line of code, not a data migration.
ALTER TABLE compositions
    ADD COLUMN youtube_id      text   NULL CHECK (youtube_id ~ '^[A-Za-z0-9_-]{11}$'),
    -- What kind of devotional piece it is (the closed list lives in the domain crate and the UI).
    ADD COLUMN devotional_form text   NULL CHECK (devotional_form IN (
        'bhajan', 'kirtan', 'stotra', 'stuti', 'aarti', 'mantra', 'shloka', 'chalisa', 'ashtakam',
        'sahasranamam', 'suprabhatam', 'keerthanam', 'namavali', 'abhang', 'shabad', 'qawwali', 'naat',
        'hamd', 'marsiya', 'hymn', 'psalm', 'carol', 'gospel', 'sutta', 'other')),
    -- The scripture, epic or text it comes from, e.g. "Bhagavad Gita 2.47" or "Ramcharitmanas".
    ADD COLUMN source_work     text   NULL CHECK (length(source_work) BETWEEN 1 AND 200),
    -- Festivals and times it is sung at, e.g. "Navratri", "Ekadashi", "morning".
    ADD COLUMN occasions       text[] NOT NULL DEFAULT '{}' CHECK (cardinality(occasions) <= 10),
    ADD CONSTRAINT compositions_devotional_structure_check
        CHECK (category = 'devotional' OR (devotional_form IS NULL AND source_work IS NULL AND occasions = '{}'));
CREATE INDEX compositions_form_idx ON compositions (devotional_form) WHERE devotional_form IS NOT NULL;

-- ---- Interface and content languages ----
UPDATE settings SET value = jsonb_set(value, '{locales}', (value -> 'locales') || '"ml"'::jsonb)
WHERE key = 'seo' AND NOT (value -> 'locales') ? 'ml';

INSERT INTO scripts (iso15924, numeric_code, name_en, is_rtl) VALUES
 ('Olck', 261, 'Ol Chiki', false), ('Mtei', 337, 'Meitei Mayek', false), ('Newa', 333, 'Newa', false)
ON CONFLICT DO NOTHING;

INSERT INTO languages (iso639_3, iso639_1, iso639_2, name_en, endonym, is_rtl) VALUES
 ('ory','or','ori','Odia','ଓଡ଼ିଆ',false), ('asm','as','asm','Assamese','অসমীয়া',false),
 ('kok',NULL,'kok','Konkani','कोंकणी',false), ('snd','sd','snd','Sindhi','سنڌي',true),
 ('kas','ks','kas','Kashmiri','कॉशुर',false), ('mai',NULL,'mai','Maithili','मैथिली',false),
 ('bho',NULL,'bho','Bhojpuri','भोजपुरी',false), ('doi',NULL,'doi','Dogri','डोगरी',false),
 ('mni',NULL,'mni','Meitei','ꯃꯤꯇꯩꯂꯣꯟ',false), ('sat',NULL,'sat','Santali','ᱥᱟᱱᱛᱟᱲᱤ',false),
 ('brx',NULL,'brx','Bodo','बड़ो',false), ('tcy',NULL,NULL,'Tulu','ತುಳು',false),
 ('raj',NULL,'raj','Rajasthani','राजस्थानी',false), ('hne',NULL,NULL,'Chhattisgarhi','छत्तीसगढ़ी',false),
 ('awa',NULL,'awa','Awadhi','अवधी',false), ('bra',NULL,'bra','Braj','ब्रजभाषा',false),
 ('pli','pi','pli','Pali','पालि',false), ('ind','id','ind','Indonesian','Bahasa Indonesia',false),
 ('msa','ms','msa','Malay','Bahasa Melayu',false), ('vie','vi','vie','Vietnamese','Tiếng Việt',false),
 ('nld','nl','nld','Dutch','Nederlands',false), ('pol','pl','pol','Polish','polski',false),
 ('ukr','uk','ukr','Ukrainian','українська',false), ('ron','ro','ron','Romanian','română',false),
 ('ces','cs','ces','Czech','čeština',false), ('hun','hu','hun','Hungarian','magyar',false),
 ('swe','sv','swe','Swedish','svenska',false), ('fil',NULL,'fil','Filipino','Filipino',false),
 ('khm','km','khm','Khmer','ខ្មែរ',false), ('mya','my','mya','Burmese','မြန်မာ',false),
 ('bod','bo','bod','Tibetan','བོད་སྐད་',false), ('lat','la','lat','Latin','latina',false)
ON CONFLICT DO NOTHING;

UPDATE languages l SET default_script_id = s.id
FROM scripts s, (VALUES
 ('ory','Orya'),('asm','Beng'),('kok','Deva'),('snd','Arab'),('kas','Arab'),('mai','Deva'),('bho','Deva'),
 ('doi','Deva'),('mni','Mtei'),('sat','Olck'),('brx','Deva'),('tcy','Knda'),('raj','Deva'),('hne','Deva'),
 ('awa','Deva'),('bra','Deva'),('pli','Deva'),('ind','Latn'),('msa','Latn'),('vie','Latn'),('nld','Latn'),
 ('pol','Latn'),('ukr','Cyrl'),('ron','Latn'),('ces','Latn'),('hun','Latn'),('swe','Latn'),('fil','Latn'),
 ('khm','Khmr'),('mya','Mymr'),('bod','Tibt'),('lat','Latn')
) AS v(lang, script) WHERE l.iso639_3 = v.lang AND s.iso15924 = v.script AND l.default_script_id IS NULL;
