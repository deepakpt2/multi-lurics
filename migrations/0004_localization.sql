-- Phase 3: reference languages/scripts, localized names, aliases, credit-role registry,
-- minimal label/publisher/genre entities. Reference data is seeded here, never hardcoded in app code.
-- Extensions are governed: only private-use codes (ISO 639-3 qaa–qtz, ISO 15924 Qaaa–Qabx), added by migration.

CREATE TABLE scripts (
    id            uuid    PRIMARY KEY DEFAULT gen_random_uuid(),
    iso15924      text    NOT NULL UNIQUE CHECK (iso15924 ~ '^[A-Z][a-z]{3}$'),
    numeric_code  integer NULL UNIQUE CHECK (numeric_code BETWEEN 0 AND 999),
    name_en       text    NOT NULL,
    is_rtl        boolean NOT NULL DEFAULT false,
    is_extension  boolean NOT NULL DEFAULT false,
    created_at    timestamptz NOT NULL DEFAULT now(),
    CHECK (is_extension = (iso15924 ~ '^Qa(a[a-z]|b[a-x])$'))
);

CREATE TABLE languages (
    id                 uuid    PRIMARY KEY DEFAULT gen_random_uuid(),
    iso639_3           text    NOT NULL UNIQUE CHECK (iso639_3 ~ '^[a-z]{3}$'),
    iso639_1           text    NULL UNIQUE CHECK (iso639_1 ~ '^[a-z]{2}$'),
    iso639_2           text    NULL CHECK (iso639_2 ~ '^[a-z]{3}$'),
    name_en            text    NOT NULL,
    endonym            text    NOT NULL,
    is_rtl             boolean NOT NULL DEFAULT false,
    default_script_id  uuid    NULL REFERENCES scripts (id),
    is_extension       boolean NOT NULL DEFAULT false,
    created_at         timestamptz NOT NULL DEFAULT now(),
    CHECK (is_extension = (iso639_3 ~ '^q[a-t][a-z]$'))
);

INSERT INTO scripts (iso15924, numeric_code, name_en, is_rtl) VALUES
 ('Latn',215,'Latin',false), ('Arab',160,'Arabic',true), ('Mlym',347,'Malayalam',false),
 ('Taml',346,'Tamil',false), ('Deva',315,'Devanagari',false), ('Telu',340,'Telugu',false),
 ('Knda',345,'Kannada',false), ('Beng',325,'Bengali',false), ('Hebr',125,'Hebrew',true),
 ('Cyrl',220,'Cyrillic',false), ('Grek',200,'Greek',false), ('Jpan',413,'Japanese',false),
 ('Hani',500,'Han',false), ('Hans',501,'Han (Simplified)',false), ('Hant',502,'Han (Traditional)',false),
 ('Kore',287,'Korean',false), ('Hang',286,'Hangul',false), ('Thai',352,'Thai',false),
 ('Gujr',320,'Gujarati',false), ('Guru',310,'Gurmukhi',false), ('Sinh',348,'Sinhala',false),
 ('Armn',230,'Armenian',false), ('Geor',240,'Georgian',false), ('Ethi',430,'Ethiopic',false),
 ('Syrc',135,'Syriac',true), ('Thaa',170,'Thaana',true), ('Orya',327,'Odia',false),
 ('Tibt',330,'Tibetan',false), ('Khmr',355,'Khmer',false), ('Laoo',356,'Lao',false),
 ('Mymr',350,'Myanmar',false);

INSERT INTO languages (iso639_3, iso639_1, iso639_2, name_en, endonym, is_rtl) VALUES
 ('eng','en','eng','English','English',false), ('ara','ar','ara','Arabic','العربية',true),
 ('mal','ml','mal','Malayalam','മലയാളം',false), ('tam','ta','tam','Tamil','தமிழ்',false),
 ('hin','hi','hin','Hindi','हिन्दी',false), ('tel','te','tel','Telugu','తెలుగు',false),
 ('kan','kn','kan','Kannada','ಕನ್ನಡ',false), ('ben','bn','ben','Bengali','বাংলা',false),
 ('urd','ur','urd','Urdu','اردو',true), ('fas','fa','fas','Persian','فارسی',true),
 ('heb','he','heb','Hebrew','עברית',true), ('fra','fr','fra','French','français',false),
 ('spa','es','spa','Spanish','español',false), ('deu','de','deu','German','Deutsch',false),
 ('jpn','ja','jpn','Japanese','日本語',false), ('kor','ko','kor','Korean','한국어',false),
 ('zho','zh','zho','Chinese','中文',false), ('rus','ru','rus','Russian','русский',false),
 ('por','pt','por','Portuguese','português',false), ('ita','it','ita','Italian','italiano',false),
 ('tur','tr','tur','Turkish','Türkçe',false), ('san','sa','san','Sanskrit','संस्कृतम्',false),
 ('pan','pa','pan','Punjabi','ਪੰਜਾਬੀ',false), ('mar','mr','mar','Marathi','मराठी',false),
 ('guj','gu','guj','Gujarati','ગુજરાતી',false), ('sin','si','sin','Sinhala','සිංහල',false),
 ('ell','el','ell','Greek','Ελληνικά',false), ('hye','hy','hye','Armenian','Հայերեն',false),
 ('kat','ka','kat','Georgian','ქართული',false), ('amh','am','amh','Amharic','አማርኛ',false),
 ('tha','th','tha','Thai','ไทย',false), ('nep','ne','nep','Nepali','नेपाली',false),
 ('swa','sw','swa','Swahili','Kiswahili',false), ('srp','sr','srp','Serbian','српски',false);

UPDATE languages l SET default_script_id = s.id
FROM scripts s, (VALUES
 ('eng','Latn'),('ara','Arab'),('mal','Mlym'),('tam','Taml'),('hin','Deva'),('tel','Telu'),('kan','Knda'),
 ('ben','Beng'),('urd','Arab'),('fas','Arab'),('heb','Hebr'),('fra','Latn'),('spa','Latn'),('deu','Latn'),
 ('jpn','Jpan'),('kor','Kore'),('zho','Hans'),('rus','Cyrl'),('por','Latn'),('ita','Latn'),('tur','Latn'),
 ('san','Deva'),('pan','Guru'),('mar','Deva'),('guj','Gujr'),('sin','Sinh'),('ell','Grek'),('hye','Armn'),
 ('kat','Geor'),('amh','Ethi'),('tha','Thai'),('nep','Deva'),('swa','Latn'),('srp','Cyrl')
) AS v(lang, script) WHERE l.iso639_3 = v.lang AND s.iso15924 = v.script;

-- ---- credit roles: registry with a display category ----
CREATE TABLE credit_roles (
    id                   uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    code                 text NOT NULL UNIQUE,
    category             text NOT NULL CHECK (category IN
                         ('original_songwriter', 'translator', 'adapter', 'performance', 'production', 'other')),
    label_en             text NOT NULL,
    default_language_id  uuid NULL REFERENCES languages (id),
    version              integer NOT NULL DEFAULT 1,
    created_at           timestamptz NOT NULL DEFAULT now()
);

INSERT INTO credit_roles (code, category, label_en, default_language_id)
SELECT v.code, v.category, v.label, (SELECT id FROM languages WHERE iso639_3 = 'eng') FROM (VALUES
 ('composer','original_songwriter','Composer'), ('lyricist','original_songwriter','Lyricist'),
 ('translator','translator','Translator'), ('adapter','adapter','Adapter'),
 ('performer','performance','Performer'), ('featured_performer','performance','Featured performer'),
 ('conductor','performance','Conductor'), ('primary_artist','performance','Primary artist'),
 ('producer','production','Producer'), ('arranger','other','Arranger')
) AS v(code, category, label);

ALTER TABLE credits DROP CONSTRAINT credits_check1;
ALTER TABLE credits ADD CONSTRAINT credits_check1 CHECK (
    (composition_id IS NOT NULL AND role IN ('composer', 'lyricist', 'translator', 'adapter'))
    OR (recording_id IS NOT NULL AND role IN ('performer', 'featured_performer', 'arranger', 'producer', 'conductor'))
    OR (album_id IS NOT NULL AND role IN ('primary_artist', 'producer'))
);
ALTER TABLE credits ADD CONSTRAINT credits_role_fkey FOREIGN KEY (role) REFERENCES credit_roles (code);

-- ---- minimal taxonomy entities (localizable) ----
CREATE TABLE labels (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(), name text NOT NULL CHECK (length(name) BETWEEN 1 AND 300),
    default_language_id uuid NULL REFERENCES languages (id), version integer NOT NULL DEFAULT 1,
    created_by uuid NOT NULL REFERENCES users (id), created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE publishers (LIKE labels INCLUDING ALL);
CREATE TABLE genres (LIKE labels INCLUDING ALL);
ALTER TABLE publishers ADD FOREIGN KEY (default_language_id) REFERENCES languages (id), ADD FOREIGN KEY (created_by) REFERENCES users (id);
ALTER TABLE genres ADD FOREIGN KEY (default_language_id) REFERENCES languages (id), ADD FOREIGN KEY (created_by) REFERENCES users (id);
CREATE INDEX labels_list_idx ON labels (created_at DESC, id DESC);
CREATE INDEX publishers_list_idx ON publishers (created_at DESC, id DESC);
CREATE INDEX genres_list_idx ON genres (created_at DESC, id DESC);

-- Every localizable entity can declare the language it was originally written in.
ALTER TABLE compositions ADD COLUMN default_language_id uuid NULL REFERENCES languages (id);
ALTER TABLE recordings   ADD COLUMN default_language_id uuid NULL REFERENCES languages (id);
ALTER TABLE artists      ADD COLUMN default_language_id uuid NULL REFERENCES languages (id);
ALTER TABLE albums       ADD COLUMN default_language_id uuid NULL REFERENCES languages (id);

-- Polymorphic references cannot be foreign keys; this guard gives the same guarantee.
CREATE FUNCTION localization_entity_exists(t text, eid uuid) RETURNS boolean
LANGUAGE sql STABLE AS $$
    SELECT CASE t
        WHEN 'composition' THEN EXISTS (SELECT 1 FROM compositions WHERE id = eid)
        WHEN 'recording'   THEN EXISTS (SELECT 1 FROM recordings WHERE id = eid)
        WHEN 'artist'      THEN EXISTS (SELECT 1 FROM artists WHERE id = eid)
        WHEN 'album'       THEN EXISTS (SELECT 1 FROM albums WHERE id = eid)
        WHEN 'label'       THEN EXISTS (SELECT 1 FROM labels WHERE id = eid)
        WHEN 'publisher'   THEN EXISTS (SELECT 1 FROM publishers WHERE id = eid)
        WHEN 'genre'       THEN EXISTS (SELECT 1 FROM genres WHERE id = eid)
        WHEN 'credit_role' THEN EXISTS (SELECT 1 FROM credit_roles WHERE id = eid)
        ELSE false END
$$;

CREATE FUNCTION localization_entity_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT localization_entity_exists(NEW.entity_type, NEW.entity_id) THEN
        RAISE EXCEPTION 'entity % % does not exist', NEW.entity_type, NEW.entity_id
            USING ERRCODE = 'foreign_key_violation', CONSTRAINT = 'localization_entity_ref';
    END IF;
    RETURN NEW;
END $$;

-- Personal names (artists) are never machine-translated and always carry a source.
CREATE TABLE entity_localizations (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_type         text NOT NULL CHECK (entity_type IN
                        ('composition','recording','artist','album','label','publisher','genre','credit_role')),
    entity_id           uuid NOT NULL,
    language_id         uuid NOT NULL REFERENCES languages (id),
    script_id           uuid NULL REFERENCES scripts (id),
    name                text NOT NULL CHECK (length(name) BETWEEN 1 AND 300),
    description         text NULL CHECK (length(description) BETWEEN 1 AND 2000),
    is_preferred        boolean NOT NULL,
    provenance          text NOT NULL CHECK (provenance IN
                        ('original','human_translation','transliteration','community','imported','machine')),
    source              text NULL CHECK (length(source) BETWEEN 1 AND 500),
    verification_state  text NOT NULL DEFAULT 'pending' CHECK (verification_state IN ('pending','verified','rejected')),
    created_by          uuid NOT NULL REFERENCES users (id),
    verified_by         uuid NULL REFERENCES users (id),
    verified_at         timestamptz NULL,
    current_revision_id uuid NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT localizations_personal_name_check CHECK (
        entity_type <> 'artist' OR (provenance <> 'machine' AND source IS NOT NULL)),
    CONSTRAINT localizations_verified_check CHECK (
        (verification_state = 'verified') = (verified_by IS NOT NULL AND verified_at IS NOT NULL)
        OR verification_state = 'rejected')
);
CREATE UNIQUE INDEX localizations_slot_key ON entity_localizations
    (entity_type, entity_id, language_id, coalesce(script_id, '00000000-0000-0000-0000-000000000000'::uuid));
-- One preferred name per entity per language.
CREATE UNIQUE INDEX localizations_preferred_key ON entity_localizations (entity_type, entity_id, language_id)
    WHERE is_preferred;
CREATE INDEX localizations_entity_idx ON entity_localizations (entity_type, entity_id) WHERE verification_state = 'verified';
CREATE INDEX localizations_queue_idx ON entity_localizations (created_at, id) WHERE verification_state = 'pending';
CREATE TRIGGER localizations_entity_guard BEFORE INSERT OR UPDATE OF entity_type, entity_id ON entity_localizations
    FOR EACH ROW EXECUTE FUNCTION localization_entity_guard();

-- Immutable history. `parent_revision_id` is unique per localization, so history stays linear
-- even if two writers race.
CREATE TABLE localization_revisions (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    localization_id   uuid NOT NULL REFERENCES entity_localizations (id),
    revision_no       integer NOT NULL CHECK (revision_no > 0),
    parent_revision_id uuid NULL REFERENCES localization_revisions (id),
    action            text NOT NULL CHECK (action IN ('created','edited','verified','rejected')),
    data              jsonb NOT NULL,
    created_by        uuid NOT NULL REFERENCES users (id),
    created_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (localization_id, revision_no),
    UNIQUE NULLS NOT DISTINCT (localization_id, parent_revision_id)
);
ALTER TABLE entity_localizations ADD CONSTRAINT localizations_current_revision_fk
    FOREIGN KEY (current_revision_id) REFERENCES localization_revisions (id);

CREATE FUNCTION localization_revisions_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'localization_revisions is append-only' USING ERRCODE = 'insufficient_privilege';
END $$;
CREATE TRIGGER localization_revisions_no_update_delete BEFORE UPDATE OR DELETE ON localization_revisions
    FOR EACH ROW EXECUTE FUNCTION localization_revisions_immutable();
CREATE TRIGGER localization_revisions_no_truncate BEFORE TRUNCATE ON localization_revisions
    FOR EACH STATEMENT EXECUTE FUNCTION localization_revisions_immutable();

-- Verified alternates (spellings, nicknames, transliterations). Source is mandatory.
CREATE TABLE entity_aliases (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_type         text NOT NULL CHECK (entity_type IN
                        ('composition','recording','artist','album','label','publisher','genre','credit_role')),
    entity_id           uuid NOT NULL,
    language_id         uuid NULL REFERENCES languages (id),
    script_id           uuid NULL REFERENCES scripts (id),
    alias               text NOT NULL CHECK (length(alias) BETWEEN 1 AND 300),
    source              text NOT NULL CHECK (length(source) BETWEEN 1 AND 500),
    verification_state  text NOT NULL DEFAULT 'pending' CHECK (verification_state IN ('pending','verified','rejected')),
    created_by          uuid NOT NULL REFERENCES users (id),
    verified_by         uuid NULL REFERENCES users (id),
    verified_at         timestamptz NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT aliases_verified_check CHECK (
        (verification_state = 'verified') = (verified_by IS NOT NULL AND verified_at IS NOT NULL)
        OR verification_state = 'rejected')
);
CREATE UNIQUE INDEX aliases_key ON entity_aliases (entity_type, entity_id, lower(alias),
    coalesce(language_id, '00000000-0000-0000-0000-000000000000'::uuid),
    coalesce(script_id, '00000000-0000-0000-0000-000000000000'::uuid));
CREATE INDEX aliases_entity_idx ON entity_aliases (entity_type, entity_id) WHERE verification_state = 'verified';
CREATE INDEX aliases_queue_idx ON entity_aliases (created_at, id) WHERE verification_state = 'pending';
CREATE TRIGGER aliases_entity_guard BEFORE INSERT OR UPDATE OF entity_type, entity_id ON entity_aliases
    FOR EACH ROW EXECUTE FUNCTION localization_entity_guard();
