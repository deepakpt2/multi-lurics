-- Identity: users, roles, server-side sessions, single-use account tokens.

CREATE TABLE users (
    id                 uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    email              citext      NOT NULL UNIQUE CHECK (length(email) BETWEEN 3 AND 254),
    display_name       text        NOT NULL CHECK (length(display_name) BETWEEN 1 AND 64),
    password_hash      text        NOT NULL,
    status             text        NOT NULL DEFAULT 'unverified'
                                   CHECK (status IN ('unverified', 'active', 'disabled')),
    email_verified_at  timestamptz NULL,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    CHECK ((status = 'unverified') = (email_verified_at IS NULL) OR status = 'disabled')
);

-- Every user is implicitly a member; only elevated roles are stored.
CREATE TABLE user_roles (
    user_id     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role        text        NOT NULL CHECK (role IN ('moderator', 'admin')),
    granted_at  timestamptz NOT NULL DEFAULT now(),
    granted_by  uuid        NULL REFERENCES users (id),
    PRIMARY KEY (user_id, role)
);

-- Only the SHA-256 of the session token is stored.
CREATE TABLE sessions (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash  bytea       NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL,
    user_agent  text        NULL
);

CREATE INDEX sessions_user_idx ON sessions (user_id);
CREATE INDEX sessions_expires_idx ON sessions (expires_at);

CREATE TABLE account_tokens (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    purpose     text        NOT NULL CHECK (purpose IN ('verify_email', 'password_reset')),
    token_hash  bytea       NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL,
    used_at     timestamptz NULL
);

CREATE INDEX account_tokens_user_idx ON account_tokens (user_id, purpose) WHERE used_at IS NULL;

ALTER TABLE audit_log
    ADD CONSTRAINT audit_log_actor_fk FOREIGN KEY (actor_id) REFERENCES users (id);
