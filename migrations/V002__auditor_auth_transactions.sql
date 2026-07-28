CREATE TABLE auditor_auth_transactions (
    id UUID PRIMARY KEY,
    grant_id UUID NOT NULL REFERENCES auditor_access_grants(id) ON DELETE CASCADE,
    state_digest BYTEA NOT NULL UNIQUE,
    nonce_digest BYTEA NOT NULL,
    pkce_verifier TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (octet_length(state_digest) = 32),
    CHECK (octet_length(nonce_digest) = 32),
    CHECK (char_length(pkce_verifier) BETWEEN 43 AND 128)
);
