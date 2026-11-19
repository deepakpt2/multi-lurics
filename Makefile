# Local equivalents of every CI step. Reads .env (see .env.example).
-include .env
export
SHELL := bash
CARGO ?= cargo
TEST_DB_URL := $(LYRICS_TEST_DATABASE_URL)
CHECK_DB_URL := $(patsubst %/lyrics_test,%/lyrics_migrate_check,$(TEST_DB_URL))

.PHONY: ci rust-ci web-ci fmt-check clippy test-rust migrate migrate-from-zero openapi web-lint web-typecheck web-test web-build api-types-check e2e dev-api dev-worker dev-web

ci: rust-ci web-ci e2e

rust-ci: fmt-check clippy test-rust migrate-from-zero
web-ci: api-types-check web-lint web-typecheck web-test web-build

fmt-check:
	$(CARGO) fmt --all --check

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

# sqlx::test creates an isolated throwaway database per test from DATABASE_URL.
test-rust:
	DATABASE_URL=$(TEST_DB_URL) $(CARGO) test --workspace

migrate:
	$(CARGO) run -q -p api -- migrate

migrate-from-zero:
	sqlx database drop -y --database-url $(CHECK_DB_URL)
	sqlx database create --database-url $(CHECK_DB_URL)
	sqlx migrate run --source migrations --database-url $(CHECK_DB_URL)
	sqlx migrate info --source migrations --database-url $(CHECK_DB_URL)
	sqlx database drop -y --database-url $(CHECK_DB_URL)

# Regenerate openapi/v1.json from code, then the TS types from it.
openapi:
	UPDATE_OPENAPI=1 $(CARGO) test -q -p api --test foundation openapi_spec_is_committed_and_current
	cd web && npm run -s gen:api

api-types-check:
	cd web && npm run -s gen:api && git diff --exit-code -- src/api/schema.d.ts

web-lint:
	cd web && npm run -s lint
web-typecheck:
	cd web && npm run -s typecheck
web-test:
	cd web && npm test
web-build:
	cd web && NEXT_TELEMETRY_DISABLED=1 npm run -s build

e2e:
	./scripts/e2e.sh

dev-api:
	$(CARGO) run -p api -- serve
dev-worker:
	$(CARGO) run -p worker
dev-web:
	cd web && npm run dev
