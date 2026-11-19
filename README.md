# Community Lyrics

Community multilingual lyrics platform. Rust (Axum/SQLx) modular monolith + Next.js frontend + PostgreSQL.

## Layout

| Path | Role |
|---|---|
| `crates/domain` | Entities, value objects, invariants. No I/O. |
| `crates/application` | Use cases: transactions (`AppTx`), authorization, audit + outbox writes, cursor pagination. |
| `crates/infrastructure` | Config (fail-fast), sqlx repositories, audit log, outbox, crypto. |
| `crates/api` | Axum HTTP: `/api/v1`, RFC 7807 problems, `RequireUser`/`RequireRole` extractors, OpenAPI. |
| `crates/worker` | Outbox consumer (dev mailbox sink for now). |
| `migrations/` | Forward-only SQL migrations (sqlx). Never edit an applied file. |
| `openapi/v1.json` | Committed API contract; frontend types are generated from it. |
| `web/` | Next.js App Router, `next-intl` (en, ar/RTL), typed client in `src/api`. |

## Setup

```sh
cp .env.example .env            # edit credentials; DB names must start with lyrics_
cargo install sqlx-cli --no-default-features --features postgres,rustls --locked
sqlx database create --database-url "$LYRICS_DATABASE_URL"
make migrate
(cd web && npm ci)
make dev-api    # :8787
make dev-worker # writes emails to .mailbox/
make dev-web    # :3000, proxies /api/v1 → API_ORIGIN (build-time, default http://127.0.0.1:8787)
```

## Checks (same as CI)

`make ci` runs everything. Individually: `make fmt-check clippy test-rust migrate-from-zero`,
`make api-types-check web-lint web-typecheck web-test web-build`, `make e2e`.
After changing an endpoint run `make openapi` and commit both generated files.

## Deploy (Docker + Traefik)

Stack: `db` (Postgres) → `migrate` (one-shot) → `api` + `worker` → `web` (only service on the Traefik network).
TLS is terminated by Cloudflare (cloudflared), so Traefik routes plain HTTP.

```sh
cp deploy/.env.example deploy/.env    # first time only; set APP_DOMAIN, APP_PUBLIC_URL, POSTGRES_PASSWORD, PROXY_NETWORK…
docker compose -f deploy/docker-compose.yml up -d --build
# update
git pull && docker compose -f deploy/docker-compose.yml up -d --build
```

All per-server values live in `deploy/.env` (gitignored); `docker-compose.yml` never needs editing.
Until SMTP exists, verification/reset emails are written to the `mailbox` volume:
`docker compose -f deploy/docker-compose.yml exec worker sh -c 'cat /data/mailbox/*.json'`.
To make the first admin:
`docker compose -f deploy/docker-compose.yml exec db psql -U lyrics -d lyrics_prod -c "INSERT INTO user_roles (user_id, role) SELECT id, 'admin' FROM users WHERE email = 'you@example.com'"`.
