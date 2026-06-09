-- Human identities (Auth0-backed), JIT-provisioned on first valid token.
CREATE TABLE IF NOT EXISTS users (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    auth0_sub   TEXT NOT NULL UNIQUE,
    email       TEXT,
    name        TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
