CREATE TABLE IF NOT EXISTS auditor_access_otps (
    id UUID PRIMARY KEY,
    grant_id UUID NOT NULL REFERENCES auditor_access_grants(id) ON DELETE CASCADE,
    code_digest BYTEA NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    failed_attempt_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (octet_length(code_digest) = 32),
    CHECK (failed_attempt_count >= 0),
    CHECK (expires_at > created_at),
    CHECK (consumed_at IS NULL OR consumed_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_auditor_access_otps_grant_created
    ON auditor_access_otps (grant_id, created_at DESC);

CREATE TABLE IF NOT EXISTS auditor_sessions (
    id UUID PRIMARY KEY,
    grant_id UUID NOT NULL REFERENCES auditor_access_grants(id) ON DELETE CASCADE,
    session_digest BYTEA NOT NULL UNIQUE,
    auditor_email TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (auditor_email = lower(trim(auditor_email))),
    CHECK (octet_length(session_digest) = 32),
    CHECK (expires_at > created_at),
    CHECK (revoked_at IS NULL OR revoked_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_auditor_sessions_active_lookup
    ON auditor_sessions (session_digest)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_auditor_sessions_grant
    ON auditor_sessions (grant_id);
