-- Foundation: extensions, append-only audit log, transactional outbox.
-- Forward-only: never edit an applied migration; add a new one instead.

CREATE EXTENSION IF NOT EXISTS citext;

CREATE TABLE audit_log (
    id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    occurred_at  timestamptz NOT NULL DEFAULT clock_timestamp(),
    actor_id     uuid        NULL,
    action       text        NOT NULL CHECK (action ~ '^[a-z_]+(\.[a-z_]+)+$'),
    target_type  text        NOT NULL,
    target_id    uuid        NULL,
    request_id   text        NULL,
    before       jsonb       NULL,
    after        jsonb       NULL,
    metadata     jsonb       NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX audit_log_occurred_idx ON audit_log (occurred_at DESC, id DESC);
CREATE INDEX audit_log_target_idx   ON audit_log (target_type, target_id, occurred_at DESC);
CREATE INDEX audit_log_actor_idx    ON audit_log (actor_id, occurred_at DESC);

CREATE FUNCTION audit_log_immutable() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'audit_log is append-only' USING ERRCODE = 'insufficient_privilege';
END;
$$;

CREATE TRIGGER audit_log_no_update_delete
    BEFORE UPDATE OR DELETE ON audit_log
    FOR EACH ROW EXECUTE FUNCTION audit_log_immutable();

CREATE TRIGGER audit_log_no_truncate
    BEFORE TRUNCATE ON audit_log
    FOR EACH STATEMENT EXECUTE FUNCTION audit_log_immutable();

CREATE TABLE outbox (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at      timestamptz NOT NULL DEFAULT clock_timestamp(),
    topic           text        NOT NULL,
    aggregate_type  text        NOT NULL,
    aggregate_id    uuid        NOT NULL,
    payload         jsonb       NOT NULL,
    available_at    timestamptz NOT NULL DEFAULT clock_timestamp(),
    attempts        integer     NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    locked_until    timestamptz NULL,
    processed_at    timestamptz NULL,
    last_error      text        NULL
);

CREATE INDEX outbox_pending_idx ON outbox (available_at, created_at) WHERE processed_at IS NULL;
