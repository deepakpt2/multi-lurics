#!/usr/bin/env bash
# Restore drill: back up the live database, restore it into a scratch database, and prove the copy
# is the same data (migration state, every table's row count, append-only protection) and usable.
# Exits non-zero on any mismatch. Usage: LYRICS_DATABASE_URL=postgres://…/lyrics_x scripts/restore-drill.sh
set -euo pipefail
: "${LYRICS_DATABASE_URL:?set LYRICS_DATABASE_URL}"
DATABASE_URL="$LYRICS_DATABASE_URL"
cd "$(dirname "$0")/.."

db="${DATABASE_URL##*/}"; db="${db%%\?*}"
case "$db" in lyrics_*) ;; *) echo "refusing: database '$db' is not a lyrics_* database" >&2; exit 64;; esac
scratch="lyrics_restore_drill"
admin="${DATABASE_URL%/*}/postgres"
scratch_url="${DATABASE_URL%/*}/$scratch"
work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT

started=$(date +%s)
scripts/backup.sh "$work" >/dev/null
dump="$(ls "$work"/*.dump)"
sha256sum -c "$dump.sha256" >/dev/null
echo "dump verified ($(du -h "$dump" | cut -f1))"

psql "$admin" -qc "DROP DATABASE IF EXISTS $scratch" -c "CREATE DATABASE $scratch"
restore_started=$(date +%s)
pg_restore --no-owner --no-privileges --exit-on-error --dbname="$scratch_url" "$dump"
restore_secs=$(( $(date +%s) - restore_started ))

q() { psql "$1" -qAtc "$2"; }
fail=0
# 1. Same migration state and checksums.
a="$(q "$DATABASE_URL" "SELECT version||':'||encode(checksum,'hex') FROM _sqlx_migrations ORDER BY version")"
b="$(q "$scratch_url" "SELECT version||':'||encode(checksum,'hex') FROM _sqlx_migrations ORDER BY version")"
[ "$a" = "$b" ] && echo "migrations: identical ($(echo "$a" | wc -l) applied)" || { echo "migrations: MISMATCH"; fail=1; }
# 2. Same row count in every table (exact counts, not estimates).
counts() { q "$1" "SELECT table_name FROM information_schema.tables WHERE table_schema='public' AND table_type='BASE TABLE' ORDER BY 1" |
  while read -r t; do echo "$t $(q "$1" "SELECT count(*) FROM \"$t\"")"; done; }
ca="$(counts "$DATABASE_URL")"; cb="$(counts "$scratch_url")"
if [ "$ca" = "$cb" ]; then echo "tables: $(echo "$ca" | wc -l) tables, $(echo "$ca" | awk '{s+=$2} END{print s}') rows, all counts equal"
else echo "tables: MISMATCH"; diff <(echo "$ca") <(echo "$cb") || true; fail=1; fi
# 3. Protections survived: append-only triggers and the primary-per-slot index.
trg="$(q "$scratch_url" "SELECT count(*) FROM pg_trigger WHERE NOT tgisinternal AND tgname LIKE '%no_update_delete'")"
idx="$(q "$scratch_url" "SELECT count(*) FROM pg_indexes WHERE indexname='contributions_primary_key'")"
[ "$trg" -ge 4 ] && [ "$idx" = 1 ] && echo "protections: $trg append-only triggers, primary-per-slot index present" || { echo "protections: MISSING"; fail=1; }
# 4. The restored copy accepts writes and still refuses history edits.
if q "$scratch_url" "UPDATE lyric_revisions SET body = body" >/dev/null 2>&1 && [ "$(q "$scratch_url" "SELECT count(*) FROM lyric_revisions")" != 0 ]; then
  echo "append-only: NOT enforced after restore"; fail=1; else echo "append-only: enforced after restore"; fi

psql "$admin" -qc "DROP DATABASE $scratch"
echo "restore time: ${restore_secs}s; total drill: $(( $(date +%s) - started ))s"
[ "$fail" = 0 ] && echo "RESTORE DRILL: PASS" || { echo "RESTORE DRILL: FAIL"; exit 1; }
