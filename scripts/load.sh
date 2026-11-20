#!/usr/bin/env bash
# Reproducible load run: release build → fresh lyrics_load DB → API + worker + Next → scripts/load.mjs.
# Usage: scripts/load.sh   (env: SONGS=300 SECS=20 CONC=16 ENFORCE=1 OUT=load-report.json SKIP_BUILD=1 to reuse builds)
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f .env ] && { set -a; . ./.env; set +a; }
: "${LYRICS_TEST_DATABASE_URL:?set LYRICS_TEST_DATABASE_URL}"
DB="${LYRICS_TEST_DATABASE_URL%/*}/lyrics_load"
API_PORT=${LOAD_API_PORT:-18281}; WEB_PORT=${LOAD_WEB_PORT:-13281}
MAILBOX=$(mktemp -d); LOGS=$(mktemp -d); PIDS=()
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; done; rm -rf "$MAILBOX"; }
trap cleanup EXIT
sqlx database drop -y --database-url "$DB" >/dev/null 2>&1 || true
sqlx database create --database-url "$DB"
if [ -z "${SKIP_BUILD:-}" ]; then
cargo build -q --release -p api -p worker
(cd web && API_ORIGIN="http://127.0.0.1:$API_PORT" NEXT_TELEMETRY_DISABLED=1 npm run -s build >"$LOGS/build.log" 2>&1) || { tail -n 30 "$LOGS/build.log"; exit 1; }
fi
export LYRICS_DATABASE_URL="$DB" LYRICS_APP_BASE_URL="http://127.0.0.1:$WEB_PORT" LYRICS_BIND_ADDR="127.0.0.1:$API_PORT" \
  LYRICS_COOKIE_SECURE=false LYRICS_DEV_MAILBOX_DIR="$MAILBOX" LYRICS_AUTH_RATE_PER_MIN=100000 ${DB_CONNS:+LYRICS_DB_MAX_CONNECTIONS=$DB_CONNS} \
  LYRICS_REVALIDATE_URL="http://127.0.0.1:$WEB_PORT/api/revalidate" LYRICS_REVALIDATE_SECRET="load-revalidate-secret"
./target/release/lyrics-api migrate >/dev/null
./target/release/lyrics-api serve >"$LOGS/api.log" 2>&1 & PIDS+=($!)
./target/release/lyrics-worker >"$LOGS/worker.log" 2>&1 & PIDS+=($!)
(cd web && API_ORIGIN="http://127.0.0.1:$API_PORT" SITE_URL="http://127.0.0.1:$WEB_PORT" REVALIDATE_SECRET="load-revalidate-secret" exec ./node_modules/.bin/next start -p "$WEB_PORT" -H 127.0.0.1 >"$LOGS/web.log" 2>&1) & PIDS+=($!)
for _ in $(seq 1 60); do
  curl -fsS "http://127.0.0.1:$API_PORT/readyz" 2>/dev/null | grep -q '"migration_version"' && curl -fsS "http://127.0.0.1:$WEB_PORT/en" 2>/dev/null | grep -q 'lang="en"' && break
  sleep 0.5
done
echo "machine: $(nproc) cores, $(free -g | awk '/Mem/{print $2}') GB; release build; Postgres shared with the load generator"
API="http://127.0.0.1:$API_PORT" WEB="http://127.0.0.1:$WEB_PORT" MAILBOX="$MAILBOX" DB="$DB" node scripts/load.mjs
