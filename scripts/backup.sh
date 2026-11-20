#!/usr/bin/env bash
# Logical backup of the application database: custom-format pg_dump, checksummed.
# Usage: LYRICS_DATABASE_URL=postgres://… scripts/backup.sh [outdir]   (default outdir: ./backups)
# Never prints the connection string. Keep dumps off the host that holds the database.
set -euo pipefail
: "${LYRICS_DATABASE_URL:?set LYRICS_DATABASE_URL}"
DATABASE_URL="$LYRICS_DATABASE_URL"
out="${1:-backups}"
mkdir -p "$out"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
file="$out/lyrics-$stamp.dump"
pg_dump --format=custom --no-owner --no-privileges --file="$file" "$DATABASE_URL"
sha256sum "$file" > "$file.sha256"
echo "backup: $file ($(du -h "$file" | cut -f1))"
