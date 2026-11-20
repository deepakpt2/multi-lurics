# Operations

One document for running Community Lyrics: release, migrations, backup and restore, incidents, dashboards,
legal workflows, capacity, limitations, and the scorecard against every phase gate.
Commands assume the compose stack in `deploy/` (`docker compose -f deploy/docker-compose.yml …`).

## Release checklist

1. `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `make test-rust`, `make web-lint web-typecheck web-test`, `make e2e` all green on the commit to ship.
2. `cargo audit` (config: `.cargo/audit.toml`) and `cd web && npm audit --omit=dev` report nothing new. Every accepted advisory is listed in the limitations register below.
3. `make openapi api-types-check`: the committed contract matches the code.
4. `scripts/restore-drill.sh` against a copy of production data passes (see Backup and restore).
5. `scripts/load.sh` on a production-like machine meets the SLOs below, or the miss is written down.
6. Read the new migrations (forward-only; see Migrations). Anything that rewrites a large table needs its own plan.
7. Take a backup (`scripts/backup.sh`), note its checksum, then `git pull && docker compose … up -d --build`.
8. After deploy: `/readyz` is 200 with the new `migration_version`; scrape `/metrics`; register a test account and confirm the verification email arrives; publish a test lyric and confirm the public page and search show it.
9. Rollback = redeploy the previous image. A migration is never rolled back; the previous release must run on the new schema (additive changes only), otherwise restore from the backup taken in step 7.

## Configuration that matters in production

| Variable | Purpose |
| --- | --- |
| `COOKIE_SECURE=true` | Session cookie is `HttpOnly; SameSite=Lax; Secure`. Never set false outside local development. |
| `SMTP_URL`, `MAIL_FROM` | Without them the worker logs that an email happened and sends nothing: nobody can verify an account. |
| `METRICS_TOKEN` | Enables `/metrics` (404 when empty). Not routed through web; scrape inside the compose network. |
| `REVALIDATE_SECRET` | Lets the worker drop cached pages when lyrics or URLs change. |
| `AUTH_RATE_PER_MIN`, `SEARCH_RATE_PER_MIN` | Per client and minute. Behind Cloudflare the client is `cf-connecting-ip`. |
| `DB_MAX_CONNECTIONS` | Per API replica. Total across replicas must stay under Postgres `max_connections`. |

## Migrations (runbook)

- Forward-only, numbered in `migrations/`. The `migrate` service runs before `api` and `worker` start; `api` serves `/readyz` 503 until the schema is current, so a half-migrated database never takes traffic.
- Write migrations so the previous release still works on the new schema: add columns nullable or with defaults, add tables and indexes, never rename or drop in the same release that stops using the old name.
- Large index builds: use `CREATE INDEX CONCURRENTLY` in a migration of its own (sqlx runs each file in a transaction unless it is a single statement marked `-- no-transaction`).
- Before applying in production: `make migrate-from-zero` (every migration applies to an empty database) and run the restore drill on a recent backup restored into staging with the new migrations applied.
- If a migration fails: the `migrate` container exits non-zero, `api` does not start, and the old containers keep serving the old schema. Fix forward with a new migration; do not edit an applied one (its checksum is recorded and the migrator refuses).

## Backup and restore

- `scripts/backup.sh [dir]`: custom-format `pg_dump` plus a SHA-256 file. Schedule it (cron on the host or a sidecar) at least daily; keep copies off the database host. Recovery point = last dump; recovery time = restore time below plus DNS/redeploy.
- `scripts/restore-drill.sh`: dumps `LYRICS_DATABASE_URL`, restores into a scratch `lyrics_restore_drill`, and fails unless migrations and checksums match, every table has the same row count, the append-only triggers and the primary-per-slot index exist, and history edits are still refused. Run it monthly and before every release that touches the schema. It refuses any database not named `lyrics_*`.
- Real restore: stop `api` and `worker`, create an empty database, `pg_restore --no-owner --dbname=… file.dump`, run `lyrics-api migrate` (applies anything newer than the backup), start the services, then check `/readyz`. Search documents, slugs and unprocessed outbox events are ordinary tables, so they return with the dump and the worker resumes where it stopped. Cached pages in `web` are not in the dump: restart `web` (or wait out the 5 minute cache) so no page outlives its data.

## Incident runbook

First look, always: `docker compose … ps`, then `curl -s http://api:8787/readyz` from inside the network, then the structured logs. Every request carries `x-request-id` (echoed in the response, in problem bodies as `request_id`, and in every log line as `request_id`): ask the reporter for it and `grep` the logs for that one id across `web`, `api` and `worker`.

### API down
`ps` shows `api` restarting or unhealthy. `logs api | tail`: exit 78 means a configuration error (the message names the variable); a migration or database error means see Database unreachable. Redeploy the previous image if a new release is the cause.

### Database unreachable
`/readyz` returns 503 `database_unavailable` and `lyrics_up_database` is 0. Check `db` is up and its disk is not full. The API recovers by itself when Postgres returns (the pool reconnects); nothing to restart. Reads of cached pages keep working at the web tier for up to 5 minutes. If the data is lost or corrupt, follow Backup and restore.

### Elevated 5xx
Group by route in the latency histogram (`status=~"5.."`). One route: read its handler's log lines by request id. All routes: database or pool (below). After a release: redeploy the previous image first, investigate second.

### Slow reads
Check `lyrics_db_pool_connections{state="idle"}`: zero idle for minutes means the pool is saturated; raise `DB_MAX_CONNECTIONS` (keep replicas × value under Postgres `max_connections`) or add API replicas. Otherwise check Postgres (`pg_stat_activity`, `pg_stat_statements` if enabled) for a slow query, and run `EXPLAIN (ANALYZE)` on it against production-sized data before adding an index.

### Outbox backlog
`lyrics_outbox_oldest_pending_seconds` is climbing. `logs worker`: a repeating `event failed` line names the topic and error. Email topics failing means SMTP is unreachable or rejecting: fix SMTP, the events retry with backoff and nothing is lost. `search.reindex` failing means a database error. A poisoned event never blocks others (each is leased and retried independently). To inspect: `SELECT topic, attempts, last_error FROM outbox WHERE processed_at IS NULL ORDER BY created_at LIMIT 20`. Scale the worker with `up -d --scale worker=2`: leases are exclusive, so replicas never process the same event concurrently.

### Abuse
Sustained 429s: find the client in the proxy logs (Cloudflare analytics shows the source). Per-client limits are in the API; for a determined attacker block at Cloudflare. Spam in content: tighten `community-settings` (admin UI/API: blocked terms, link limits, rate limits) without a deploy; hide with the moderation queue.

### Content incidents
Pending or private text visible publicly is a severity-1 incident: take the affected route offline at the proxy, then find the leak with the isolation suite (`cargo test -p api --test isolation`), fix, and purge `web` caches by restarting it. Record it in the audit log notes and tell the affected contributors.

## Dashboards (build these in Grafana from the Prometheus metrics)

1. **Traffic and errors**: request rate by route template; 4xx and 5xx rate; 429 rate.
2. **Latency**: p50/p95/p99 by route group (public read, search, typeahead, write) with the SLO lines from the capacity table.
3. **Database**: pool total/idle, `lyrics_up_database`, Postgres connections and slow queries (postgres exporter).
4. **Worker**: outbox pending, retrying, oldest pending age.
5. **Moderation and legal** (from SQL, via Grafana's Postgres data source): open reports, open legal notices, open appeals, oldest age of each. These are queue-health numbers, not alerts.
6. **Edge**: Cloudflare cache hit ratio, origin 5xx.

Alert rules: `deploy/alerts.yml` (each carries a runbook link to the sections above).

## Legal workflows

- **Policies** are public pages in both locales: `/legal/terms`, `/legal/privacy`, `/legal/content-policy`, linked from every page footer. They are working drafts: have a lawyer review them before launch (register item L1).
- **Notice** (copyright complaint, other content complaint, privacy request): `/legal/notice` or `POST /api/v1/legal/notices`. No account needed; rate limited per client; the filer must confirm good faith; only a *published* contribution can be named (an unknown and a pending id answer the same, so a notice cannot probe for private submissions). The filer gets a reference (`LN-…`); contact details are visible only to staff.
- **Decision** (moderator, `/moderation/legal`): `takedown` withdraws the public pointer of the contribution (revisions are untouched), promotes the oldest remaining published alternative if it was the primary, queues search/cache invalidation, and is audited as `contribution.taken_down`. `reject` or `close` record the outcome. A notice is decided once (409 otherwise). Privacy requests cannot take content down; handle the data request by hand (below) and `close` with a note.
- **Appeal**: only the contributor of a taken-down work (`POST /api/v1/legal/notices/{id}/appeal`; anyone else sees 404). One open appeal per notice. An **admin who did not decide the takedown** decides it (`/api/v1/admin/legal/appeals`); upholding republishes exactly the withdrawn revision (as an alternative if the slot got a new primary) and audits `contribution.reinstated`. Refused staff attempts are audited as `moderation.denied`.
- **Privacy request handling (manual, target 30 days)**: export = the user's row, contributions, comments, ratings, bookmarks, reports (SQL, by user id). Erasure = delete the account's personal data (email, display name, sessions) and keep published contributions under an anonymised author so revision history stays intact; record the action in the audit log. Notices from the filer are kept for 24 months as evidence, then deleted.
- **Counter-notice for non-contributors** is not modelled (register item L2).

## Capacity and SLOs

Reproduce: `scripts/load.sh` (release build, fresh `lyrics_load` database, API + worker + Next, 300 seeded songs, 20 s per workload, 16 concurrent clients; writes `load-report.json`; `ENFORCE=1` exits non-zero on a miss). Raw result of the run recorded here: `docs/load-report.json`.

Machine: 5 cores, 6 GB, Postgres and the load generator on the same box (so numbers are pessimistic for the server and optimistic for the network). Release build.

| Workload | SLO (p95) | Measured p95 | p50 | Throughput | Errors |
| --- | --- | --- | --- | --- | --- |
| Public read (`GET /contributions/{id}`, `/recordings/{id}/contributions`) | 150 ms | 8.0 ms | 3.7 ms | 3,647 req/s | 0 |
| Search (`GET /search`) | 300 ms | 267 ms | 193 ms | 86 req/s | 0 |
| Typeahead (`GET /search/suggest`, many client IPs) | 100 ms | 79.5 ms | 54 ms | 310 req/s | 0 |
| Write (`POST /compositions`, 6 signed-in writers) | 400 ms | 17.5 ms | 8.3 ms | 625 req/s | 0 |
| Public page (`/en/song/{slug}`, warm ISR) | 400 ms | 269 ms | 223 ms | 71 req/s | 0 |
| Public page, first render (cold ISR, 4 at a time) | none | 113 ms | 70 ms | — | 0 |

Error budget: under 0.5% non-429 errors per workload. Run-to-run variance on this shared box is about ±30% on search and typeahead (three runs: search p95 236–309 ms, typeahead 79–129 ms), so the two tightest SLOs are met with little margin here and should be re-measured on production hardware.

**What was measured before changing anything.** Search was the slowest API workload. Its candidate query takes about 3 ms on its own (`EXPLAIN ANALYZE`, 1,299 documents), and raising the database pool from 10 to 30 connections did not help (search p95 236 → 260 ms, typeahead 87 → 125 ms), so neither the SQL nor the pool is the bottleneck on this box: with 16 clients on 5 cores shared with Postgres, Node and Next, latency is CPU contention. No optimisation was made: there is no measured bottleneck to fix. The first run in which three workloads missed their SLO ran while compile jobs were using the same cores; it is not used.

### Decision: Redis and a dedicated search engine
**Not adopted.** Postgres met every SLO at 300 songs / 1,299 search documents with the working set in memory; nothing measured points at Postgres as the limit. Revisit when any of these is true on production hardware: search p95 above 300 ms for a week at under 50% CPU; `search_documents` above ~500k rows with a sequential-scan plan on the candidate query (the trigram GIN index exists and will be chosen at size; confirm with `EXPLAIN`); public read p95 above 150 ms because of database load rather than pool size. First steps before a new service: more API replicas, a read replica for search, then (only if still needed) a dedicated engine behind the existing `SearchService` trait, which exists so the swap does not touch handlers.

## Scaling, caching and CDN

- **Stateless tiers.** `web`, `api` and `worker` hold no state beyond what is described here; sessions are database rows and the cookie is an opaque token. They scale independently: `docker compose … up -d --scale api=3 --scale worker=2` (no fixed container names or host ports). Rate-limit buckets are in-process, so with N API replicas a client may get up to N times the limit.
- **Never cached:** every API response that depends on the caller or on unpublished state sends `Cache-Control: private, no-store` (lyrics, comparison, community, legal, moderation, admin); search responses that could differ per caller are tested for it, and 404/429/5xx are never cached. `cargo test -p api --test isolation` fails if pending text appears in any public API, search, suggest, sitemap, or comparison response, or if those routes become cacheable.
- **Cached on purpose:** public pages (Next ISR, 5 minute revalidate) and public API reads that carry `public, max-age`. Invalidation is explicit: the worker calls `/api/revalidate` (shared secret) with tags when lyrics are published, unpublished, renamed or reindexed; a failed call is retried, never dropped. A takedown therefore removes a page from `web` within seconds, and from the CDN as soon as the CDN honours `s-maxage`/purge (see register item C1).
- **CDN (Cloudflare).** Cache only `GET /en|ar/song/*`, `/sitemap*`, `/robots.txt` and static assets. Bypass `/api/*`, `/metrics` and anything with the session cookie.

## Security review

- **Sessions and CSRF:** opaque server-side session ids in an `HttpOnly; SameSite=Lax; Secure` cookie; state-changing requests also need `X-Requested-With` (a custom header cannot be sent cross-origin without a CORS preflight, which the API never grants); tested.
- **Abuse:** per-client token bucket on login/register/verify/password reset and legal notices (`AUTH_RATE_PER_MIN`), typeahead (`SEARCH_RATE_PER_MIN`), per-user action limits with Postgres advisory locks for community writes, spam heuristics and length limits (configurable).
- **Uploads:** one endpoint accepts files, `POST /api/v1/images` (signed-in, CSRF header, body limited to `MEDIA_MAX_BYTES`). The type is decided from the content (JPEG, PNG, WebP, GIF only; SVG and anything else is refused), dimensions are capped (`MEDIA_MAX_SIDE`, decoder memory limit), and the image is decoded and re-encoded to square JPEGs, so EXIF/GPS and embedded payloads are dropped. Stored names are server-generated UUIDs (never the uploaded name) and served with `nosniff` and a sandbox CSP. A person may hold 5 unsent and 10 pending images.
- **Output escaping:** React escapes all text; JSON-LD is serialised with `<` escaped; lyric text is rendered as text with explicit `dir`/`lang`, never as HTML.
- **Dependencies:** `npm audit --omit=dev` found a high PostCSS advisory (via Next); fixed with an `overrides` pin to PostCSS 8.5.33, now 0 findings. `cargo audit` reports two advisories that are in `Cargo.lock` only because `sqlx-macros` resolves every database driver (`rsa` via sqlx-mysql, `libsqlite3-sys` via sqlx-sqlite); `cargo tree -i` shows neither is compiled into any binary, so they are ignored in `.cargo/audit.toml` with that reason. `chrono` is flagged unmaintained (no fix exists; register item D1).
- **Secrets:** SMTP credentials live only in `LYRICS_SMTP_URL`; the URL, session tokens, verification and reset links are never logged.
- **Metrics:** labels are route templates, never raw paths, ids or query strings; the endpoint needs a bearer token and is not routed through `web`.
- **Authorization suite:** `cargo test -p api --test isolation` walks the OpenAPI contract: every operation marked secured must answer 401 to an anonymous caller; no plain member may succeed on any `/moderation/` or `/admin/` operation; the list of anonymously writable endpoints is pinned, so a new unauthenticated write fails the build until reviewed; operation ids must be unique (a duplicate silently merged two endpoints' client types once).

## Accessibility audit

Automated: `web/src/test/a11y.test.tsx` runs axe-core (WCAG 2.0/2.1 A and AA plus best-practice rules) in English and Arabic over the notice form (also in its error state), the four-panel comparison with an RTL panel, the legal moderation queue, and header plus footer navigation: zero violations. Not automatable in jsdom and checked by reading the CSS: logical properties only (mirrors in RTL), focus styles on interactive elements, the skip link, form errors announced with `role="alert"`, labels bound to every input, per-text `dir`/`lang`. **Not yet done:** screen-reader pass, measured colour contrast, and touch-target sizing on real devices (register item A1).

## Known limitations register

| ID | Limitation | Impact | Mitigation / when to fix |
| --- | --- | --- | --- |
| L1 | Terms, privacy and content policy are drafts | Legal exposure | Lawyer review before launch |
| L2 | No counter-notice from non-contributors; no UI for the contributor's takedown appeal (API only) | Appeals need an API client | Add UI when the first takedown happens |
| L3 | Privacy export/erasure is a manual procedure | Slow, error-prone | Automate if volume justifies |
| C1 | Next ISR cache is per `web` replica; the worker's revalidate call reaches one replica | With several `web` replicas a stale page can outlive a takedown up to 5 minutes | Run one `web` replica, or add a shared cache handler / CDN purge by tag before scaling `web` |
| C2 | Rate-limit buckets are per API replica | Limits loosen with replicas | Move to Postgres or Redis counters if abuse is seen |
| M1 | Metrics are in-process and reset on restart; no tracing | Short history on restart | Prometheus keeps history; add OpenTelemetry if cross-service tracing is needed |
| M2 | Alert thresholds and dashboards are untested against live traffic | Possible noise or gaps | Tune in the first week; rules are in `deploy/alerts.yml` |
| B1 | Backups are logical dumps (RPO = backup interval), schedule is not automated here | Data since the last dump is lost | Add WAL archiving / PITR when RPO must be minutes |
| B2 | Restore drill measured at 3 MB / 45k rows (1 s); restore time at production size is unknown | RTO estimate unreliable | Re-run the drill on production-size data |
| P1 | Load numbers come from one 5-core shared box with 300 songs | Production headroom unknown | Re-run `scripts/load.sh` with larger `SONGS` on production-like hardware |
| P2 | Search and typeahead SLOs have little margin on this box | Possible misses on weaker hardware | See the Redis/search decision for next steps |
| U1 | Images are stored on local disk (one host) | Not shared across hosts; a lost volume loses images | Swap `MediaStore` for an S3-compatible store before running API replicas on several hosts; no malware scan beyond re-encoding |
| D1 | `chrono` unmaintained; two lockfile-only advisories ignored | None known | Revisit at each release |
| A1 | Accessibility: no manual screen-reader or contrast audit | Unknown real-world issues | Before launch |
| W1 | Reputation scores are computed on read (sum of events) | Cost grows with event count | Add a summary table + worker job only if profile reads become slow |
| X1 | Docker image build and CI workflow not executed in this environment | Deploy files unproven here | Run `docker compose build` and push to CI before launch |
| X2 | Moderators bypass reputation thresholds; ratings, reactions, bookmarks are not audited; a saved comparison keeps unavailable panels | Carried from Phase 8 | As noted in the Phase 8 report |

## Scorecard against every phase exit gate

Evidence is the automated suite run at this commit (Rust workspace all green, web 130/130, e2e 8/8) unless noted. "Held" means the gate's checks still pass; it does not mean nothing is left (see the register).

| Phase | Gate | Status | Evidence / caveat |
| --- | --- | --- | --- |
| 0 | Operating rules, repo skeleton, CI definitions | Held | `make ci` targets exist; CI workflow and Docker build were never executed in this environment (X1) |
| 1 | Foundation, identity, API contract | Held | auth/foundation suites; contract test `openapi_spec_is_committed_and_current`; new: unique operation ids, anonymous-write allow-list |
| 2 | Catalogue CRUD, optimistic concurrency | Held | catalogue suites incl. stale-write 409 and unauthorized cases |
| 3 | Localization, language ≠ script | Held | localization suites |
| 4 | Contributions, immutable revisions, primary-per-slot | Held | lyrics suites; DB triggers survive restore (drill) |
| 5 | Entity suggestion and merge | Held | entity suites; no merge UI (carried) |
| 6 | Public multilingual search | Held | search suites; typeahead limiter; SLOs met (capacity table) |
| 7 | SEO, slugs, ISR, invalidation | Held | seo suites + e2e; per-replica ISR cache limitation (C1) |
| 8 | Comparison and community | Held | community suite 14/14 + e2e; gaps as in the Phase 8 report (X2) |
| 9 | Production readiness | See the report | Metrics, auth rate limits, SMTP delivery, legal workflow, backup + restore drill, load script and SLO table, isolation/authz suite, accessibility audit (automated), runbooks, alerts, register |

## Browse (`/browse`)

Public, read-only, published songs only. `GET /api/v1/browse/songs`, `/browse/facets/{facet}` and `/browse/suggest` take filters as `facet=slug` pairs (AND across all of them); the web address carries the same pairs, so a view is shareable and survives refresh and back/forward. Facets (artist, singer, lyricist, composer, album, raga, deity, religion, category, language, tag) are defined once, in `infrastructure::browse::Facet`; the web mirror is `FACET_KEYS` in `web/src/lib/browse.ts` and the generated API types fail the build if the two drift.

- **Slugs, never ids.** Artists and albums are addressed by a slug of their canonical name (`browse_slugs`, migration 0014): `yesudas`, a second person with the same name gets `yesudas-2`. A rename retires the old slug but it keeps resolving (merged entities follow `entity_merges`), and the page redirects to the current slug. The worker keeps slugs current on the `search.reindex` event; a list that meets an entity without one creates it on the spot (one small insert, idempotent).
- **Suggestions** reuse `search_documents`, so localized names, transliterations and aliases match and resolve to the one entity. With filters active, a suggestion is offered only if a published song has it *and* matches every active filter.
- **Cost.** Lists use `OFFSET` paging (page size at most 50) with `count(*) OVER()`; each filter is an `EXISTS` over indexed credit/track joins (`credits_artist_role_idx`, `album_tracks_recording_idx`). Answers carry ETag and `max-age` from the `search_ranking.cache_max_age_secs` setting, and suggestions share the typeahead rate limit. Watch `lyrics` request latency for `/browse/`; if the unfiltered total becomes slow on a very large catalogue, cache the unfiltered count rather than changing the query.

### Ragas, deities and tags are entities (migration 0015)
A song's raga, deity and tags point at catalogue entities (`ragas`, `deities`, `tags`) that have localizations, transliterations, aliases and merge, like artists and albums. Saving a song with text still works: the text is matched to an existing entity by normalized name, verified localization or alias, or by a **phonetic key** (`lyrics_phonetic_key`: Malayalam, Devanagari, Tamil, Telugu and Kannada are romanized and loosely folded, so "Mohanam" and "മോഹനം" meet); only when nothing matches is a new entity created. The key is deliberately loose, so two names that no rule connects stay separate until a moderator merges them (`POST /entities/merges`, which moves the songs). The migration folded existing text values that sound alike into one entity each. Browse and typeahead use the same key, so typing in one script finds the other.

## Images (migration 0016)

Artists, albums, labels, publishers, genres, ragas and deities can have one public image. Anyone signed in can upload one from the browse lists, a song page's credits, or while proposing a new entity; it is reviewed like lyrics: moderators and contributors the lyrics trust policy auto-publishes are published immediately, everyone else waits in **Moderation → Images**. A new image never replaces the public one before approval; approving it retires the old one and deletes its files. Images sent with a new-entity proposal are decided with the proposal. Merging entities moves the merged entity's image to the survivor if the survivor has none.

| Variable | Default | Meaning |
|---|---|---|
| `MEDIA_DIR` (`LYRICS_MEDIA_DIR`) | `/data/media` in compose | Where files live. Persistent volume; include it in backups. |
| `MEDIA_MAX_BYTES` | 5242880 | Largest upload. |
| `MEDIA_MAX_SIDE` | 8000 | Largest width or height accepted. |
| `MEDIA_SIZES` | `96,240,640` | Square sizes generated per image (16–4096, at most 8). A request is served the smallest size that fits. Changing it affects new uploads only. |

Files are at `<MEDIA_DIR>/<image id>/<size>.jpg`. `entity_images` is the record; a file without a row (a crash between the two) is harmless and can be deleted. Unsent drafts (uploaded but never attached to a proposal) are not purged automatically yet; they are capped at 5 per person.
