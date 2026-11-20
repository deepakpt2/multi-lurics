-- Articles: blog-style pages written by moderators (celebrations, special occasions, announcements).
-- The text is Markdown written elsewhere and pasted or uploaded; it is rendered with the site's own
-- styling. An article can be put on the home page for a period (`on_home`, optionally between
-- `home_from` and `home_until`); outside it the home page shows nothing for articles.
CREATE TABLE articles (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    slug          text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) <= 80),
    title         text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    summary       text NULL CHECK (length(summary) BETWEEN 1 AND 500),
    body_md       text NOT NULL CHECK (length(body_md) BETWEEN 1 AND 200000),
    cover_image_id uuid NULL,
    status        text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published')),
    published_at  timestamptz NULL,
    on_home       boolean NOT NULL DEFAULT false,
    home_from     timestamptz NULL,
    home_until    timestamptz NULL,
    created_by    uuid NOT NULL REFERENCES users (id),
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    version       integer NOT NULL DEFAULT 1,
    CONSTRAINT articles_published_check CHECK ((status = 'published') = (published_at IS NOT NULL)),
    CONSTRAINT articles_window_check CHECK (home_from IS NULL OR home_until IS NULL OR home_from < home_until)
);
CREATE INDEX articles_published_idx ON articles (published_at DESC, id) WHERE status = 'published';

-- Pictures for articles. They keep their proportions (unlike entity images, which are square crops)
-- and are public as soon as they are uploaded: only moderators can upload them.
CREATE TABLE article_images (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    article_id uuid NOT NULL REFERENCES articles (id) ON DELETE CASCADE,
    width      integer NOT NULL CHECK (width > 0),
    height     integer NOT NULL CHECK (height > 0),
    sizes      integer[] NOT NULL CHECK (cardinality(sizes) > 0),
    credit     text NULL CHECK (length(credit) BETWEEN 1 AND 300),
    created_by uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX article_images_article_idx ON article_images (article_id, created_at);
ALTER TABLE articles ADD CONSTRAINT articles_cover_fk FOREIGN KEY (cover_image_id) REFERENCES article_images (id) ON DELETE SET NULL;
