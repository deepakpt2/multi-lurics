#!/usr/bin/env bash
# Full-stack smoke: fresh lyrics_e2e DB → migrate → API + worker + Next → e2e tests.
# Requires: .env (or LYRICS_TEST_DATABASE_URL in env), cargo, sqlx-cli, node + `npm ci` in web/.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f .env ] && { set -a; . ./.env; set +a; }

: "${LYRICS_TEST_DATABASE_URL:?set LYRICS_TEST_DATABASE_URL}"
E2E_DB_URL="${LYRICS_TEST_DATABASE_URL%/*}/lyrics_e2e"
API_PORT=${E2E_API_PORT:-18181}
WEB_PORT=${E2E_WEB_PORT:-13181}
MAILBOX=$(mktemp -d)
LOGS=$(mktemp -d)
PIDS=()
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; done; rm -rf "$MAILBOX"; }
trap cleanup EXIT

sqlx database drop -y --database-url "$E2E_DB_URL" >/dev/null
sqlx database create --database-url "$E2E_DB_URL"
cargo build -q -p api -p worker
# Next bakes rewrites in at build time, so the proxy target must be set here.
(cd web && API_ORIGIN="http://127.0.0.1:$API_PORT" NEXT_TELEMETRY_DISABLED=1 npm run -s build >"$LOGS/build.log" 2>&1) \
  || { tail -n 30 "$LOGS/build.log"; exit 1; }

export LYRICS_DATABASE_URL="$E2E_DB_URL" LYRICS_APP_BASE_URL="http://127.0.0.1:$WEB_PORT" \
  LYRICS_BIND_ADDR="127.0.0.1:$API_PORT" LYRICS_COOKIE_SECURE=false LYRICS_DEV_MAILBOX_DIR="$MAILBOX"
./target/debug/lyrics-api migrate
./target/debug/lyrics-api serve >"$LOGS/api.log" 2>&1 & PIDS+=($!)
./target/debug/lyrics-worker >"$LOGS/worker.log" 2>&1 & PIDS+=($!)
(cd web && API_ORIGIN="http://127.0.0.1:$API_PORT" exec ./node_modules/.bin/next start -p "$WEB_PORT" -H 127.0.0.1 >"$LOGS/web.log" 2>&1) & PIDS+=($!)

# Each probe must match content only our services return, so a foreign process on the port can't pass.
wait_for() {
  for _ in $(seq 1 60); do
    for p in "${PIDS[@]}"; do kill -0 "$p" 2>/dev/null || { echo "a service exited early"; tail -n 20 "$LOGS"/*.log; exit 1; }; done
    curl -fsS "$1" 2>/dev/null | grep -q "$2" && return 0
    sleep 0.5
  done
  echo "timeout: $1"; tail -n 20 "$LOGS"/*.log; exit 1
}
wait_for "http://127.0.0.1:$API_PORT/readyz" '"migration_version"'
wait_for "http://127.0.0.1:$WEB_PORT/en" 'lang="en"'

(cd web && E2E_BASE_URL="http://127.0.0.1:$WEB_PORT" E2E_MAILBOX="$MAILBOX" E2E_DB_URL="$E2E_DB_URL" npm run -s test:e2e)
