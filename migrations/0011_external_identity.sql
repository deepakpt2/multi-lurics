-- Sign-in with an external identity provider (Google). A provider account is identified by its
-- stable `sub`, never by email, so a changed or recycled email cannot take over an account.

CREATE TABLE external_identities (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider    text        NOT NULL CHECK (provider IN ('google')),
    subject     text        NOT NULL CHECK (length(subject) BETWEEN 1 AND 255),
    email       citext      NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (provider, subject),
    UNIQUE (user_id, provider)
);

-- One row per in-flight sign-in. Single use; only the hash of `state` is stored.
CREATE TABLE oauth_states (
    state_hash     bytea       PRIMARY KEY CHECK (length(state_hash) = 32),
    code_verifier  text        NOT NULL,
    nonce          text        NOT NULL,
    locale         text        NOT NULL CHECK (locale IN ('en', 'ar')),
    created_at     timestamptz NOT NULL DEFAULT now(),
    expires_at     timestamptz NOT NULL
);

CREATE INDEX oauth_states_expires_idx ON oauth_states (expires_at);
