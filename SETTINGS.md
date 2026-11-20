# Settings reference

Everything an operator can configure, and **where** to change it. There are three places:

| Where | Changed by | Takes effect |
|---|---|---|
| **Environment** (`.env`, container env) | Whoever deploys | After restarting the API / worker / web |
| **Admin screens and API** | Admins (some: moderators) | At once |
| **The `settings` table** (Postgres rows) | SQL only: **no screen yet** | When the row is next read (see each entry) |

Never print or commit real values for secrets.

---

## 1. Environment variables

### API and worker (`LYRICS_*`)

Copy `.env.example` to `.env`. A bad or incomplete value stops start-up with a message naming it.

| Variable | Default | What it does |
|---|---|---|
| `LYRICS_DATABASE_URL` | required | Postgres URL. The database name **must** start with `lyrics_` (start-up refuses anything else). Percent-encode special characters in the password. |
| `LYRICS_TEST_DATABASE_URL` | tests only | Used by `make test` / `sqlx::test`; each test gets a throwaway database. |
| `LYRICS_APP_BASE_URL` | required | Public address of the site (links in emails, cookies). `http` or `https` only. |
| `LYRICS_BIND_ADDR` | `127.0.0.1:8787` | Where the API listens. |
| `LYRICS_SESSION_TTL_HOURS` | `336` (14 days) | How long a sign-in lasts. |
| `LYRICS_COOKIE_SECURE` | `true` | Set `false` only for plain-http local development. |
| `LYRICS_DB_MAX_CONNECTIONS` | `10` | Database pool size. |
| `LYRICS_LOG_JSON` | off | `1` for JSON logs. |
| `RUST_LOG` | `info,sqlx=warn` | Log level. |
| `LYRICS_ADMIN_EMAIL`, `LYRICS_ADMIN_PASSWORD`, `LYRICS_ADMIN_NAME` | none / `Administrator` | First administrator, created or promoted at start-up. Email and password must be set together. |

### Mail

| Variable | What it does |
|---|---|
| `LYRICS_SMTP_URL`, `LYRICS_MAIL_FROM` | Where the worker sends email (verification, password reset). |
| `LYRICS_DEV_MAILBOX_DIR` | Development: the worker writes emails to this folder instead of sending. |

### Limits (per client address, in memory per API server)

| Variable | Default | Covers |
|---|---|---|
| `LYRICS_SEARCH_RATE_PER_MIN` | `120` | Typeahead and view counting. |
| `LYRICS_AUTH_RATE_PER_MIN` | `20` | Sign-up, sign-in, password reset, legal notices. |

These rely on `CF-Connecting-IP` or `X-Forwarded-For` set by your proxy. Per-person limits are in the admin screen (section 2).

### Sign-in with Google

`LYRICS_GOOGLE_CLIENT_ID` and `LYRICS_GOOGLE_CLIENT_SECRET`. Set both or neither. The button appears only when both are set.

### Bot check (Cloudflare Turnstile)

`LYRICS_TURNSTILE_SITE_KEY` (public) and `LYRICS_TURNSTILE_SECRET_KEY` (private). Set both or neither: one alone stops start-up. When set, sign-up, sign-in, password reset and legal notices need a solved challenge. If Cloudflare cannot be reached the form is refused, not let through. Unset means no check.

### Images

| Variable | Default | What it does |
|---|---|---|
| `LYRICS_MEDIA_DIR` | `./media-data` | Where files are stored. **Must be on a persistent volume.** |
| `LYRICS_MEDIA_MAX_BYTES` | `5242880` (5 MB) | Largest upload (10 KB to 50 MB allowed). |
| `LYRICS_MEDIA_MAX_SIDE` | `8000` | Largest image side in pixels. |
| `LYRICS_MEDIA_SIZES` | `96,240,640` | Square sizes made for entity images (ascending). Article pictures use widths 480, 960 and 1600 (not configurable). |

### Home page lists and monitoring

| Variable | Default | What it does |
|---|---|---|
| `LYRICS_HOME_REFRESH_SECS` | `3600` | How often the worker recomputes the popular and new lists. |
| `LYRICS_METRICS_TOKEN` | none | Token that unlocks the metrics endpoint. |

### Cache invalidation (API to web)

| Variable | What it does |
|---|---|
| `LYRICS_REVALIDATE_URL` | The web app's `/api/revalidate` address. |
| `LYRICS_REVALIDATE_SECRET` | Shared secret sent with it. |

### Web app (Next.js)

| Variable | Default | What it does |
|---|---|---|
| `API_ORIGIN` | `http://127.0.0.1:8787` | Where the API is. **Read at build time** for the `/api/v1` proxy rewrite; also read at run time for server fetches. |
| `SITE_URL` | falls back to `LYRICS_APP_BASE_URL`, then `http://localhost:3000` | Public address used in canonical links, hreflang, sitemaps, robots. Read at run time. |
| `REVALIDATE_SECRET` | none | Must match `LYRICS_REVALIDATE_SECRET`. |
| `NEXT_OUTPUT_STANDALONE` | off | `1` builds the self-contained bundle for the Docker image. |

---

## 2. Admin screens and API

| What | Where | Who |
|---|---|---|
| **Community limits and rules**: reputation points, thresholds, reputation levels, text-length limits, spam rules (links, repeats, blocked terms), and the per-person rate limits for `comment`, `rating`, `reaction`, `bookmark`, `report`, `appeal`, `comparison`, `contribute` (30 per hour) and `upload` (20 per hour). Moderators and admins are exempt from `contribute` and `upload`. | Admin screen `/admin/community-settings` · `GET/PUT /api/v1/admin/community-settings` | Admin |
| **Lyrics trust policy**: which roles publish lyrics without review, and how many published lyrics make a member trusted (null disables it). Default: moderators and admins only. | `GET/PUT /api/v1/admin/trust-policy` | Admin |
| **Users and roles** | `/admin/users` | Admin |
| **Metadata values** (singers, albums, ragas, deities, tags in advance) | `/admin/metadata` · `POST /api/v1/admin/metadata` | Admin |
| **Bulk import of songs** | `/import` · `POST /api/v1/import/songs` | Moderator |
| **Articles**: write, publish, put on the home page for a period | `/articles`, `/articles/new`, each article's own page | Moderator |
| **Editor's picks** (songs featured on the home page) | The pick button on a song page · `PUT/DELETE /api/v1/editors-picks/{id}` | Moderator |
| **Moderation queues**: lyrics, images, reports, appeals, legal notices | `/moderation` | Moderator |
| **Audit log** | `GET /api/v1/admin/audit-events` | Admin |

---

## 3. The `settings` table (SQL only)

No screen or API yet for these. Each is one row: `settings(key, value jsonb)`. Change with `UPDATE settings SET value = ... WHERE key = '...'`, ideally inside a transaction, and keep the shape shown. A malformed value makes the feature fail with an internal error until fixed.

### `seo`: page addresses, languages, sitemap

```json
{
  "locales": ["en", "ar", "ml"],
  "default_locale": "en",
  "slug": {"preserve_native_script": false, "ascii_fold_latin": true, "max_length": 80, "separator": "-"},
  "sitemap_page_size": 5000
}
```

| Field | Meaning |
|---|---|
| `locales` | Site languages that get their own pages (`/en/...`, `/ml/...`). A locale has a page for a song only if the song has a verified title in that language, or it is the default. |
| `default_locale` | Fallback language, and the one used as the "English" source for addresses. |
| `slug.preserve_native_script` | `false` (default): addresses are Latin letters only, made from the English title, the same in every language; a song whose main title is not Latin gets `id-<8 hex>` until it has an English title. `true`: each language's address uses that title's own script. |
| `slug.ascii_fold_latin` | Fold accented Latin letters (é becomes e). |
| `slug.max_length`, `slug.separator` | Length cap and word separator. |
| `slug.reserved` | Words never used as a bare address (defaults include `new`, `edit`, `admin`, `api`, `search`, `login`, `register`, `account`). |
| `sitemap_page_size` | URLs per sitemap file. |

**When it takes effect:** the worker applies it to a page the next time that page is re-indexed (an edit or an event). To re-slug everything, queue a `search.reindex` outbox event for each song, artist and album, as migration `0022_latin_slugs.sql` does. Old addresses stay as history and redirect to the current one.

Example, allow native-script addresses again:

```sql
UPDATE settings
   SET value = jsonb_set(value, '{slug,preserve_native_script}', 'true')
 WHERE key = 'seo';
```

### `search_ranking`: how search orders results

```json
{
  "class_weights": {"exact": 100, "normalized": 90, "prefix": 70, "fuzzy": 40},
  "origin_multipliers": {"name": 1.0, "localization": 1.0, "transliteration": 0.95, "alias": 0.9},
  "type_bonus": {"composition": 4, "recording": 3, "album": 2, "artist": 1},
  "popularity_weight": 2.0,
  "popularity_cap": 10.0,
  "language_boost": 5.0,
  "min_suggest_chars": 2,
  "cache_max_age_secs": 30
}
```

Whole-text lyric matches are ranked in code below title matches: opening line 60, start of a line 50, inside a line 30 (not in this row).

---

### Languages for lyrics and titles

These are rows in the `languages` and `scripts` tables (seeded by migrations `0004` and `0019`, including Sanskrit with Devanagari as its usual script). Language pickers search them as you type (`GET /api/v1/languages?q=sans`), and only a language chosen from the suggestions is accepted. To add one, add a migration that inserts the language and its usual script; no code change is needed. Adding a language the *site itself* is shown in (menus, `/ml/...` pages) is a separate, larger change: see `web/src/i18n/routing.ts` and the `seo` setting above.

## 4. Fixed in code (not configurable)

Changing these needs a code change and a release.

- **Contribution limits:** at most 200 names per metadata batch, 60 pictures per article, 16 MB for imports, 8 MB for a contributed song, body limits on uploads (`LYRICS_MEDIA_MAX_BYTES` plus 64 KB).
- **Articles:** at most three on the home page at once (newest first); body up to 200,000 characters; slugs `a-z`, digits, hyphens, up to 80 characters; `new` and `home` are reserved.
- **Home page lists:** the ranking formula (views, ratings, comments over 30 days) lives in `crates/infrastructure/src/home.rs`; how often it runs is `LYRICS_HOME_REFRESH_SECS`.
- **Security headers** (`web/next.config.ts`): `Referrer-Policy`, `X-Content-Type-Options`, `X-Frame-Options: DENY`, `Strict-Transport-Security`, `Permissions-Policy`. There is no Content-Security-Policy yet; it needs testing against the YouTube embed and Turnstile in a browser first.
- **Site languages offered in the footer switcher:** English and Malayalam. Arabic is complete but not offered. Malayalam translates part of the text and shows English for the rest.

---

## 5. Not built yet

- An admin screen for the `seo` and `search_ranking` rows, including a "re-slug all pages" button.
- A Content-Security-Policy.
- Turnstile on signed-in forms (contributions and comments rely on the per-person limits instead).
