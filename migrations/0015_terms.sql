-- Phonetic keys: one loose Latin spelling for any text, so "Mohanam" and "മോഹനം" (and Devanagari, Tamil,
-- Telugu, Kannada spellings) meet. Used to link a typed value to an existing entity and to search across scripts.
-- Deliberately loose (long/short vowels, aspirates and doubled letters are folded): it proposes and matches,
-- it never replaces a name. Display values are untouched.
CREATE TABLE translit_map (
    ch    text PRIMARY KEY,
    kind  char(1) NOT NULL CHECK (kind IN ('C', 'V', 'S', 'X', 'M')),  -- consonant, vowel, vowel sign, virama, mark
    latin text NOT NULL
);
INSERT INTO translit_map (ch, kind, latin) VALUES
('ँ','M','m'),
('ं','M','m'),
('ः','M','h'),
('अ','V','a'),
('आ','V','aa'),
('इ','V','i'),
('ई','V','ii'),
('उ','V','u'),
('ऊ','V','uu'),
('ऋ','V','ri'),
('ऌ','V','li'),
('ऎ','V','e'),
('ए','V','e'),
('ऐ','V','ai'),
('ऒ','V','o'),
('ओ','V','o'),
('औ','V','au'),
('क','C','k'),
('ख','C','kh'),
('ग','C','g'),
('घ','C','gh'),
('ङ','C','n'),
('च','C','c'),
('छ','C','ch'),
('ज','C','j'),
('झ','C','jh'),
('ञ','C','n'),
('ट','C','t'),
('ठ','C','th'),
('ड','C','d'),
('ढ','C','dh'),
('ण','C','n'),
('त','C','t'),
('थ','C','th'),
('द','C','d'),
('ध','C','dh'),
('न','C','n'),
('ऩ','C','n'),
('प','C','p'),
('फ','C','ph'),
('ब','C','b'),
('भ','C','bh'),
('म','C','m'),
('य','C','y'),
('र','C','r'),
('ऱ','C','r'),
('ल','C','l'),
('ळ','C','l'),
('ऴ','C','z'),
('व','C','v'),
('श','C','s'),
('ष','C','s'),
('स','C','s'),
('ह','C','h'),
('ा','S','aa'),
('ि','S','i'),
('ी','S','ii'),
('ु','S','u'),
('ू','S','uu'),
('ृ','S','ri'),
('ॄ','S','ri'),
('ॆ','S','e'),
('े','S','e'),
('ै','S','ai'),
('ॊ','S','o'),
('ो','S','o'),
('ौ','S','au'),
('्','X',''),
('ॗ','S','au'),
('஁','M','m'),
('ஂ','M','m'),
('ஃ','M','h'),
('அ','V','a'),
('ஆ','V','aa'),
('இ','V','i'),
('ஈ','V','ii'),
('உ','V','u'),
('ஊ','V','uu'),
('஋','V','ri'),
('஌','V','li'),
('எ','V','e'),
('ஏ','V','e'),
('ஐ','V','ai'),
('ஒ','V','o'),
('ஓ','V','o'),
('ஔ','V','au'),
('க','C','k'),
('஖','C','kh'),
('஗','C','g'),
('஘','C','gh'),
('ங','C','n'),
('ச','C','c'),
('஛','C','ch'),
('ஜ','C','j'),
('஝','C','jh'),
('ஞ','C','n'),
('ட','C','t'),
('஠','C','th'),
('஡','C','d'),
('஢','C','dh'),
('ண','C','n'),
('த','C','t'),
('஥','C','th'),
('஦','C','d'),
('஧','C','dh'),
('ந','C','n'),
('ன','C','n'),
('ப','C','p'),
('஫','C','ph'),
('஬','C','b'),
('஭','C','bh'),
('ம','C','m'),
('ய','C','y'),
('ர','C','r'),
('ற','C','r'),
('ல','C','l'),
('ள','C','l'),
('ழ','C','z'),
('வ','C','v'),
('ஶ','C','s'),
('ஷ','C','s'),
('ஸ','C','s'),
('ஹ','C','h'),
('ா','S','aa'),
('ி','S','i'),
('ீ','S','ii'),
('ு','S','u'),
('ூ','S','uu'),
('௃','S','ri'),
('௄','S','ri'),
('ெ','S','e'),
('ே','S','e'),
('ை','S','ai'),
('ொ','S','o'),
('ோ','S','o'),
('ௌ','S','au'),
('்','X',''),
('ௗ','S','au'),
('ఁ','M','m'),
('ం','M','m'),
('ః','M','h'),
('అ','V','a'),
('ఆ','V','aa'),
('ఇ','V','i'),
('ఈ','V','ii'),
('ఉ','V','u'),
('ఊ','V','uu'),
('ఋ','V','ri'),
('ఌ','V','li'),
('ఎ','V','e'),
('ఏ','V','e'),
('ఐ','V','ai'),
('ఒ','V','o'),
('ఓ','V','o'),
('ఔ','V','au'),
('క','C','k'),
('ఖ','C','kh'),
('గ','C','g'),
('ఘ','C','gh'),
('ఙ','C','n'),
('చ','C','c'),
('ఛ','C','ch'),
('జ','C','j'),
('ఝ','C','jh'),
('ఞ','C','n'),
('ట','C','t'),
('ఠ','C','th'),
('డ','C','d'),
('ఢ','C','dh'),
('ణ','C','n'),
('త','C','t'),
('థ','C','th'),
('ద','C','d'),
('ధ','C','dh'),
('న','C','n'),
('఩','C','n'),
('ప','C','p'),
('ఫ','C','ph'),
('బ','C','b'),
('భ','C','bh'),
('మ','C','m'),
('య','C','y'),
('ర','C','r'),
('ఱ','C','r'),
('ల','C','l'),
('ళ','C','l'),
('ఴ','C','z'),
('వ','C','v'),
('శ','C','s'),
('ష','C','s'),
('స','C','s'),
('హ','C','h'),
('ా','S','aa'),
('ి','S','i'),
('ీ','S','ii'),
('ు','S','u'),
('ూ','S','uu'),
('ృ','S','ri'),
('ౄ','S','ri'),
('ె','S','e'),
('ే','S','e'),
('ై','S','ai'),
('ొ','S','o'),
('ో','S','o'),
('ౌ','S','au'),
('్','X',''),
('౗','S','au'),
('ಁ','M','m'),
('ಂ','M','m'),
('ಃ','M','h'),
('ಅ','V','a'),
('ಆ','V','aa'),
('ಇ','V','i'),
('ಈ','V','ii'),
('ಉ','V','u'),
('ಊ','V','uu'),
('ಋ','V','ri'),
('ಌ','V','li'),
('ಎ','V','e'),
('ಏ','V','e'),
('ಐ','V','ai'),
('ಒ','V','o'),
('ಓ','V','o'),
('ಔ','V','au'),
('ಕ','C','k'),
('ಖ','C','kh'),
('ಗ','C','g'),
('ಘ','C','gh'),
('ಙ','C','n'),
('ಚ','C','c'),
('ಛ','C','ch'),
('ಜ','C','j'),
('ಝ','C','jh'),
('ಞ','C','n'),
('ಟ','C','t'),
('ಠ','C','th'),
('ಡ','C','d'),
('ಢ','C','dh'),
('ಣ','C','n'),
('ತ','C','t'),
('ಥ','C','th'),
('ದ','C','d'),
('ಧ','C','dh'),
('ನ','C','n'),
('಩','C','n'),
('ಪ','C','p'),
('ಫ','C','ph'),
('ಬ','C','b'),
('ಭ','C','bh'),
('ಮ','C','m'),
('ಯ','C','y'),
('ರ','C','r'),
('ಱ','C','r'),
('ಲ','C','l'),
('ಳ','C','l'),
('಴','C','z'),
('ವ','C','v'),
('ಶ','C','s'),
('ಷ','C','s'),
('ಸ','C','s'),
('ಹ','C','h'),
('ಾ','S','aa'),
('ಿ','S','i'),
('ೀ','S','ii'),
('ು','S','u'),
('ೂ','S','uu'),
('ೃ','S','ri'),
('ೄ','S','ri'),
('ೆ','S','e'),
('ೇ','S','e'),
('ೈ','S','ai'),
('ೊ','S','o'),
('ೋ','S','o'),
('ೌ','S','au'),
('್','X',''),
('೗','S','au'),
('ഁ','M','m'),
('ം','M','m'),
('ഃ','M','h'),
('അ','V','a'),
('ആ','V','aa'),
('ഇ','V','i'),
('ഈ','V','ii'),
('ഉ','V','u'),
('ഊ','V','uu'),
('ഋ','V','ri'),
('ഌ','V','li'),
('എ','V','e'),
('ഏ','V','e'),
('ഐ','V','ai'),
('ഒ','V','o'),
('ഓ','V','o'),
('ഔ','V','au'),
('ക','C','k'),
('ഖ','C','kh'),
('ഗ','C','g'),
('ഘ','C','gh'),
('ങ','C','n'),
('ച','C','c'),
('ഛ','C','ch'),
('ജ','C','j'),
('ഝ','C','jh'),
('ഞ','C','n'),
('ട','C','t'),
('ഠ','C','th'),
('ഡ','C','d'),
('ഢ','C','dh'),
('ണ','C','n'),
('ത','C','t'),
('ഥ','C','th'),
('ദ','C','d'),
('ധ','C','dh'),
('ന','C','n'),
('ഩ','C','n'),
('പ','C','p'),
('ഫ','C','ph'),
('ബ','C','b'),
('ഭ','C','bh'),
('മ','C','m'),
('യ','C','y'),
('ര','C','r'),
('റ','C','r'),
('ല','C','l'),
('ള','C','l'),
('ഴ','C','z'),
('വ','C','v'),
('ശ','C','s'),
('ഷ','C','s'),
('സ','C','s'),
('ഹ','C','h'),
('ാ','S','aa'),
('ി','S','i'),
('ീ','S','ii'),
('ു','S','u'),
('ൂ','S','uu'),
('ൃ','S','ri'),
('ൄ','S','ri'),
('െ','S','e'),
('േ','S','e'),
('ൈ','S','ai'),
('ൊ','S','o'),
('ോ','S','o'),
('ൌ','S','au'),
('്','X',''),
('ൎ','V','r'),
('ൗ','S','au'),
('ൺ','V','n'),
('ൻ','V','n'),
('ർ','V','r'),
('ൽ','V','l'),
('ൾ','V','l'),
('ൿ','V','k');

-- `final_strip` drops a word-final schwa; a half-typed word (typeahead prefix) must keep it.
CREATE FUNCTION lyrics_phonetic_key(s text, final_strip boolean DEFAULT true) RETURNS text LANGUAGE plpgsql STABLE STRICT AS $$
DECLARE r text;
BEGIN
    -- NFC keeps two-part Malayalam vowel signs whole; NFKD (used by lyrics_search_key) would split them.
    SELECT string_agg(
               CASE m.kind WHEN 'C' THEN m.latin || '~' WHEN 'S' THEN '^' || m.latin WHEN 'X' THEN '^'
                           WHEN 'V' THEN m.latin ELSE coalesce(m.latin, t.ch) END,
               '' ORDER BY t.ord)
      INTO r
      FROM regexp_split_to_table(lower(normalize(s, NFC)), '') WITH ORDINALITY AS t(ch, ord)
      LEFT JOIN translit_map m ON m.ch = t.ch;
    -- A consonant carries an inherent "a" unless a vowel sign or virama follows.
    r := replace(replace(replace(coalesce(r, ''), '~^', ''), '~', 'a'), '^', '');
    r := lyrics_search_key(r);
    r := replace(r, 'w', 'v');
    r := regexp_replace(r, '([kgcjtdpbs])h', '\1', 'g');
    r := replace(r, 'h', '');
    r := regexp_replace(r, 'aa+', 'a', 'g');
    r := regexp_replace(r, '(ee|ii)+', 'i', 'g');
    r := regexp_replace(r, '(oo|uu)+', 'u', 'g');
    r := regexp_replace(r, '([a-z])\1+', '\1', 'g');
    IF final_strip THEN
        r := regexp_replace(r, '([a-z]{2,})a(\s|$)', '\1\2', 'g');   -- word-final schwa
    END IF;
    RETURN replace(r, ' ', '');
END $$;

-- ---- ragas, deities and tags become catalogue entities ----
-- Until now these were free text on a song, so "Mohanam" and "മോഹനം" were two values. As entities they have
-- localizations, transliterations and aliases like every other name, can be merged, and a song points at one.
CREATE TABLE ragas (LIKE genres INCLUDING ALL);
CREATE TABLE deities (LIKE genres INCLUDING ALL);
CREATE TABLE tags (LIKE genres INCLUDING ALL);
ALTER TABLE ragas    ADD FOREIGN KEY (default_language_id) REFERENCES languages (id), ADD FOREIGN KEY (created_by) REFERENCES users (id);
ALTER TABLE deities  ADD FOREIGN KEY (default_language_id) REFERENCES languages (id), ADD FOREIGN KEY (created_by) REFERENCES users (id);
ALTER TABLE tags     ADD FOREIGN KEY (default_language_id) REFERENCES languages (id), ADD FOREIGN KEY (created_by) REFERENCES users (id);
CREATE INDEX ragas_list_idx ON ragas (created_at DESC, id DESC);
CREATE INDEX deities_list_idx ON deities (created_at DESC, id DESC);
CREATE INDEX tags_list_idx ON tags (created_at DESC, id DESC);

CREATE OR REPLACE FUNCTION localization_entity_exists(t text, eid uuid) RETURNS boolean
LANGUAGE sql STABLE AS $$
    SELECT CASE t
        WHEN 'composition' THEN EXISTS (SELECT 1 FROM compositions WHERE id = eid)
        WHEN 'recording'   THEN EXISTS (SELECT 1 FROM recordings WHERE id = eid)
        WHEN 'artist'      THEN EXISTS (SELECT 1 FROM artists WHERE id = eid)
        WHEN 'album'       THEN EXISTS (SELECT 1 FROM albums WHERE id = eid)
        WHEN 'label'       THEN EXISTS (SELECT 1 FROM labels WHERE id = eid)
        WHEN 'publisher'   THEN EXISTS (SELECT 1 FROM publishers WHERE id = eid)
        WHEN 'genre'       THEN EXISTS (SELECT 1 FROM genres WHERE id = eid)
        WHEN 'raga'        THEN EXISTS (SELECT 1 FROM ragas WHERE id = eid)
        WHEN 'deity'       THEN EXISTS (SELECT 1 FROM deities WHERE id = eid)
        WHEN 'tag'         THEN EXISTS (SELECT 1 FROM tags WHERE id = eid)
        WHEN 'credit_role' THEN EXISTS (SELECT 1 FROM credit_roles WHERE id = eid)
        ELSE false END
$$;

ALTER TABLE entity_localizations DROP CONSTRAINT entity_localizations_entity_type_check;
ALTER TABLE entity_localizations ADD CONSTRAINT entity_localizations_entity_type_check CHECK (entity_type IN
    ('composition','recording','artist','album','label','publisher','genre','raga','deity','tag','credit_role'));
ALTER TABLE entity_aliases DROP CONSTRAINT entity_aliases_entity_type_check;
ALTER TABLE entity_aliases ADD CONSTRAINT entity_aliases_entity_type_check CHECK (entity_type IN
    ('composition','recording','artist','album','label','publisher','genre','raga','deity','tag','credit_role'));
ALTER TABLE entity_proposals DROP CONSTRAINT entity_proposals_entity_type_check;
ALTER TABLE entity_proposals ADD CONSTRAINT entity_proposals_entity_type_check CHECK (entity_type IN
    ('artist','album','label','publisher','genre','raga','deity','tag'));
ALTER TABLE entity_merges DROP CONSTRAINT entity_merges_entity_type_check;
ALTER TABLE entity_merges ADD CONSTRAINT entity_merges_entity_type_check CHECK (entity_type IN
    ('artist','album','label','publisher','genre','raga','deity','tag'));
ALTER TABLE browse_slugs DROP CONSTRAINT browse_slugs_entity_type_check;
ALTER TABLE browse_slugs ADD CONSTRAINT browse_slugs_entity_type_check CHECK (entity_type IN
    ('artist','album','raga','deity','tag'));

-- ---- search_keys: add the phonetic key; cover the new types ----
ALTER TABLE search_keys DROP CONSTRAINT search_keys_entity_type_check;
ALTER TABLE search_keys ADD CONSTRAINT search_keys_entity_type_check CHECK (entity_type IN
    ('artist','album','label','publisher','genre','raga','deity','tag','language','script'));
ALTER TABLE search_keys ADD COLUMN phon text NOT NULL DEFAULT '';
CREATE INDEX search_keys_phon_idx ON search_keys (entity_type, phon) WHERE phon <> '';

CREATE OR REPLACE FUNCTION search_keys_refresh(t text, eid uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE canonical text[];
BEGIN
    canonical := CASE t
        WHEN 'artist'    THEN ARRAY(SELECT name FROM artists WHERE id = eid)
        WHEN 'album'     THEN ARRAY(SELECT title FROM albums WHERE id = eid)
        WHEN 'label'     THEN ARRAY(SELECT name FROM labels WHERE id = eid)
        WHEN 'publisher' THEN ARRAY(SELECT name FROM publishers WHERE id = eid)
        WHEN 'genre'     THEN ARRAY(SELECT name FROM genres WHERE id = eid)
        WHEN 'raga'      THEN ARRAY(SELECT name FROM ragas WHERE id = eid)
        WHEN 'deity'     THEN ARRAY(SELECT name FROM deities WHERE id = eid)
        WHEN 'tag'       THEN ARRAY(SELECT name FROM tags WHERE id = eid)
        WHEN 'language'  THEN ARRAY(SELECT x FROM languages l, unnest(ARRAY[l.name_en, l.endonym, l.iso639_3, l.iso639_1]) x WHERE l.id = eid)
        WHEN 'script'    THEN ARRAY(SELECT x FROM scripts s, unnest(ARRAY[s.name_en, s.iso15924]) x WHERE s.id = eid)
    END;
    DELETE FROM search_keys WHERE entity_type = t AND entity_id = eid;
    INSERT INTO search_keys (entity_type, entity_id, origin, key, compact, phon)
    SELECT t, eid, o, k, replace(k, ' ', ''), lyrics_phonetic_key(disp)
    FROM (
        SELECT 1 AS prio, 'name' AS o, lyrics_search_key(v) AS k, v AS disp FROM unnest(canonical) v WHERE v IS NOT NULL
        UNION ALL
        SELECT 2, 'localization', lyrics_search_key(name), name FROM entity_localizations
            WHERE entity_type = t AND entity_id = eid AND verification_state = 'verified'
        UNION ALL
        SELECT 3, 'alias', lyrics_search_key(alias), alias FROM entity_aliases
            WHERE entity_type = t AND entity_id = eid AND verification_state = 'verified'
    ) s
    WHERE k <> ''
    ORDER BY prio
    ON CONFLICT DO NOTHING;
END $$;

CREATE OR REPLACE FUNCTION search_keys_child_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'DELETE' AND NEW.entity_type IN ('artist','album','label','publisher','genre','raga','deity','tag') THEN
        PERFORM search_keys_refresh(NEW.entity_type, NEW.entity_id);
    END IF;
    IF TG_OP <> 'INSERT' AND OLD.entity_type IN ('artist','album','label','publisher','genre','raga','deity','tag')
       AND (TG_OP = 'DELETE' OR (OLD.entity_type, OLD.entity_id) IS DISTINCT FROM (NEW.entity_type, NEW.entity_id)) THEN
        PERFORM search_keys_refresh(OLD.entity_type, OLD.entity_id);
    END IF;
    RETURN NULL;
END $$;

CREATE TRIGGER ragas_search AFTER INSERT OR UPDATE OF name OR DELETE ON ragas
    FOR EACH ROW EXECUTE FUNCTION search_keys_entity_trigger('raga');
CREATE TRIGGER deities_search AFTER INSERT OR UPDATE OF name OR DELETE ON deities
    FOR EACH ROW EXECUTE FUNCTION search_keys_entity_trigger('deity');
CREATE TRIGGER tags_search AFTER INSERT OR UPDATE OF name OR DELETE ON tags
    FOR EACH ROW EXECUTE FUNCTION search_keys_entity_trigger('tag');

-- Existing names gain their phonetic key.
SELECT search_keys_refresh('artist', id) FROM artists;
SELECT search_keys_refresh('album', id) FROM albums;
SELECT search_keys_refresh('label', id) FROM labels;
SELECT search_keys_refresh('publisher', id) FROM publishers;
SELECT search_keys_refresh('genre', id) FROM genres;

-- ---- songs point at one raga, one deity and any number of tags ----
ALTER TABLE compositions ADD COLUMN raga_id uuid NULL REFERENCES ragas (id), ADD COLUMN deity_id uuid NULL REFERENCES deities (id);
CREATE TABLE composition_tags (
    composition_id uuid    NOT NULL REFERENCES compositions (id),
    tag_id         uuid    NOT NULL REFERENCES tags (id),
    position       integer NOT NULL CHECK (position >= 0),
    PRIMARY KEY (composition_id, tag_id)
);
CREATE INDEX composition_tags_tag_idx ON composition_tags (tag_id, composition_id);
CREATE INDEX compositions_raga_idx ON compositions (raga_id) WHERE raga_id IS NOT NULL;
CREATE INDEX compositions_deity_idx ON compositions (deity_id) WHERE deity_id IS NOT NULL;

-- Backfill: spellings that sound the same become one entity (the most used spelling is its name).
INSERT INTO ragas (name, created_by)
SELECT DISTINCT ON (p) name, created_by FROM (
    SELECT lyrics_phonetic_key(raga) AS p, raga AS name, created_by,
           count(*) OVER (PARTITION BY lyrics_phonetic_key(raga), raga) AS n, created_at
    FROM compositions WHERE raga IS NOT NULL) s
WHERE p <> '' ORDER BY p, n DESC, created_at;
UPDATE compositions c SET raga_id = r.id FROM ragas r
    WHERE c.raga IS NOT NULL AND lyrics_phonetic_key(r.name) = lyrics_phonetic_key(c.raga) AND lyrics_phonetic_key(c.raga) <> '';

INSERT INTO deities (name, created_by)
SELECT DISTINCT ON (p) name, created_by FROM (
    SELECT lyrics_phonetic_key(deity) AS p, deity AS name, created_by,
           count(*) OVER (PARTITION BY lyrics_phonetic_key(deity), deity) AS n, created_at
    FROM compositions WHERE deity IS NOT NULL) s
WHERE p <> '' ORDER BY p, n DESC, created_at;
UPDATE compositions c SET deity_id = d.id FROM deities d
    WHERE c.deity IS NOT NULL AND lyrics_phonetic_key(d.name) = lyrics_phonetic_key(c.deity) AND lyrics_phonetic_key(c.deity) <> '';

INSERT INTO tags (name, created_by)
SELECT DISTINCT ON (p) name, created_by FROM (
    SELECT lyrics_phonetic_key(t) AS p, t AS name, c.created_by,
           count(*) OVER (PARTITION BY lyrics_phonetic_key(t), t) AS n, c.created_at
    FROM compositions c, unnest(c.tags) AS t) s
WHERE p <> '' ORDER BY p, n DESC, created_at;
INSERT INTO composition_tags (composition_id, tag_id, position)
SELECT DISTINCT ON (c.id, g.id) c.id, g.id, (x.ord - 1)::int
FROM compositions c, unnest(c.tags) WITH ORDINALITY AS x(t, ord)
JOIN tags g ON lyrics_phonetic_key(g.name) = lyrics_phonetic_key(x.t)
WHERE lyrics_phonetic_key(x.t) <> ''
ORDER BY c.id, g.id, x.ord;

DROP TRIGGER compositions_search_docs ON compositions;   -- watches the columns about to go; recreated below
ALTER TABLE compositions DROP CONSTRAINT compositions_devotional_check;
DROP INDEX compositions_tags_idx;
DROP INDEX IF EXISTS compositions_raga_key_idx;
DROP INDEX IF EXISTS compositions_deity_key_idx;
ALTER TABLE compositions DROP COLUMN raga, DROP COLUMN deity, DROP COLUMN tags;
ALTER TABLE compositions ADD CONSTRAINT compositions_devotional_check
    CHECK (category = 'devotional' OR (religion IS NULL AND deity_id IS NULL));

-- ---- search_documents: the new types, the phonetic key, and a song's raga/deity/tags as metadata ----
ALTER TABLE search_documents DROP CONSTRAINT search_documents_entity_type_check;
ALTER TABLE search_documents ADD CONSTRAINT search_documents_entity_type_check CHECK (entity_type IN
    ('composition','recording','artist','album','raga','deity','tag'));
ALTER TABLE search_documents ADD COLUMN phon text NOT NULL DEFAULT '';
CREATE INDEX search_documents_phon_idx ON search_documents (entity_type, phon) WHERE phon <> '';

CREATE OR REPLACE FUNCTION search_documents_refresh(t text, eid uuid) RETURNS void LANGUAGE plpgsql AS $$
DECLARE canonical text; pop integer; writer boolean;
BEGIN
    DELETE FROM search_documents WHERE entity_type = t AND entity_id = eid;
    canonical := CASE t
        WHEN 'artist'      THEN (SELECT name  FROM artists      WHERE id = eid)
        WHEN 'album'       THEN (SELECT title FROM albums       WHERE id = eid)
        WHEN 'composition' THEN (SELECT title FROM compositions WHERE id = eid)
        WHEN 'recording'   THEN (SELECT title FROM recordings   WHERE id = eid)
        WHEN 'raga'        THEN (SELECT name  FROM ragas        WHERE id = eid)
        WHEN 'deity'       THEN (SELECT name  FROM deities      WHERE id = eid)
        WHEN 'tag'         THEN (SELECT name  FROM tags         WHERE id = eid)
    END;
    IF canonical IS NULL THEN RETURN; END IF;   -- deleted (for example merged away)

    pop := CASE t
        WHEN 'recording' THEN (SELECT count(*) FROM contributions
                               WHERE recording_id = eid AND published_revision_id IS NOT NULL)
        WHEN 'composition' THEN (SELECT count(*) FROM contributions c JOIN recordings r ON r.id = c.recording_id
                                 WHERE r.composition_id = eid AND c.published_revision_id IS NOT NULL)
        WHEN 'album' THEN (SELECT count(*) FROM contributions c JOIN album_tracks a ON a.recording_id = c.recording_id
                           WHERE a.album_id = eid AND c.published_revision_id IS NOT NULL)
        WHEN 'artist' THEN (SELECT count(*) FROM contributions c JOIN recordings r ON r.id = c.recording_id
                            WHERE c.published_revision_id IS NOT NULL AND EXISTS (
                                SELECT 1 FROM credits x WHERE x.artist_id = eid
                                  AND (x.recording_id = r.id OR x.composition_id = r.composition_id)))
        WHEN 'raga' THEN (SELECT count(*) FROM compositions c WHERE c.raga_id = eid)
        WHEN 'deity' THEN (SELECT count(*) FROM compositions c WHERE c.deity_id = eid)
        WHEN 'tag' THEN (SELECT count(*) FROM composition_tags c WHERE c.tag_id = eid)
    END;
    writer := t = 'artist' AND EXISTS (SELECT 1 FROM credits WHERE artist_id = eid AND composition_id IS NOT NULL);

    INSERT INTO search_documents (entity_type, entity_id, key, compact, display, origin, language_id, script_id,
                                  source_id, popularity, contributor, phon)
    SELECT t, eid, k.key, replace(k.key, ' ', ''), s.disp, s.o, s.lang, s.scr, s.src, pop, writer, lyrics_phonetic_key(s.disp)
    FROM (
        SELECT 1 AS prio, 'name' AS o, canonical AS disp, NULL::uuid AS lang, NULL::uuid AS scr, NULL::uuid AS src
        UNION ALL
        SELECT 2, CASE WHEN provenance = 'transliteration' THEN 'transliteration' ELSE 'localization' END,
               name, language_id, script_id, id
        FROM entity_localizations
        WHERE entity_type = t AND entity_id = eid AND verification_state = 'verified'
        UNION ALL
        SELECT 3, 'alias', alias, language_id, script_id, id
        FROM entity_aliases
        WHERE entity_type = t AND entity_id = eid AND verification_state = 'verified'
        UNION ALL
        -- Raga, deity and tags make a song findable ("Hanuman", "Mohanam", "bhajan").
        SELECT 4, 'metadata', v, NULL::uuid, NULL::uuid, NULL::uuid
        FROM compositions c, LATERAL unnest(ARRAY[
            (SELECT name FROM ragas WHERE id = c.raga_id), (SELECT name FROM deities WHERE id = c.deity_id)]
            || ARRAY(SELECT g.name FROM composition_tags ct JOIN tags g ON g.id = ct.tag_id WHERE ct.composition_id = c.id)) AS v
        WHERE t = 'composition' AND c.id = eid AND v IS NOT NULL
    ) s
    CROSS JOIN LATERAL (SELECT lyrics_search_key(s.disp) AS key) k
    WHERE k.key <> ''
    ORDER BY s.prio
    ON CONFLICT DO NOTHING;
END $$;

CREATE TRIGGER compositions_search_docs AFTER INSERT OR UPDATE OF title, raga_id, deity_id OR DELETE ON compositions
    FOR EACH ROW EXECUTE FUNCTION search_documents_entity_trigger('composition');

CREATE TRIGGER ragas_search_docs AFTER INSERT OR UPDATE OF name OR DELETE ON ragas
    FOR EACH ROW EXECUTE FUNCTION search_documents_entity_trigger('raga');
CREATE TRIGGER deities_search_docs AFTER INSERT OR UPDATE OF name OR DELETE ON deities
    FOR EACH ROW EXECUTE FUNCTION search_documents_entity_trigger('deity');
CREATE TRIGGER tags_search_docs AFTER INSERT OR UPDATE OF name OR DELETE ON tags
    FOR EACH ROW EXECUTE FUNCTION search_documents_entity_trigger('tag');

CREATE OR REPLACE FUNCTION search_documents_child_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'DELETE' AND NEW.entity_type IN ('composition', 'recording', 'artist', 'album', 'raga', 'deity', 'tag') THEN
        PERFORM search_enqueue(NEW.entity_type, NEW.entity_id);
    END IF;
    IF TG_OP <> 'INSERT' AND OLD.entity_type IN ('composition', 'recording', 'artist', 'album', 'raga', 'deity', 'tag') THEN
        PERFORM search_enqueue(OLD.entity_type, OLD.entity_id);
    END IF;
    RETURN NULL;
END $$;

-- A song's tags changed: its search documents change with them.
CREATE FUNCTION search_documents_song_tag_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP <> 'DELETE' THEN PERFORM search_enqueue('composition', NEW.composition_id); END IF;
    IF TG_OP <> 'INSERT' THEN PERFORM search_enqueue('composition', OLD.composition_id); END IF;
    RETURN NULL;
END $$;
CREATE TRIGGER composition_tags_search_docs AFTER INSERT OR UPDATE OR DELETE ON composition_tags
    FOR EACH ROW EXECUTE FUNCTION search_documents_song_tag_trigger();

-- Renaming or merging a raga, deity or tag changes the songs that use it.
CREATE FUNCTION search_documents_term_rename_trigger() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM search_enqueue('composition', c.id) FROM compositions c
        WHERE (TG_ARGV[0] = 'raga' AND c.raga_id = NEW.id) OR (TG_ARGV[0] = 'deity' AND c.deity_id = NEW.id)
        OR (TG_ARGV[0] = 'tag' AND EXISTS (SELECT 1 FROM composition_tags t WHERE t.composition_id = c.id AND t.tag_id = NEW.id));
    RETURN NULL;
END $$;
CREATE TRIGGER ragas_rename_docs AFTER UPDATE OF name ON ragas FOR EACH ROW EXECUTE FUNCTION search_documents_term_rename_trigger('raga');
CREATE TRIGGER deities_rename_docs AFTER UPDATE OF name ON deities FOR EACH ROW EXECUTE FUNCTION search_documents_term_rename_trigger('deity');
CREATE TRIGGER tags_rename_docs AFTER UPDATE OF name ON tags FOR EACH ROW EXECUTE FUNCTION search_documents_term_rename_trigger('tag');

-- Rebuild every document (phonetic keys, new types).
SELECT search_enqueue(t, id) FROM (
    SELECT 'artist' AS t, id FROM artists UNION ALL SELECT 'album', id FROM albums
    UNION ALL SELECT 'composition', id FROM compositions UNION ALL SELECT 'recording', id FROM recordings
    UNION ALL SELECT 'raga', id FROM ragas UNION ALL SELECT 'deity', id FROM deities UNION ALL SELECT 'tag', id FROM tags) x;
