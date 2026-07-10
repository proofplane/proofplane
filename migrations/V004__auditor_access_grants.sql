INSERT INTO workspace_permissions (permission)
VALUES ('manage_auditor_access')
ON CONFLICT DO NOTHING;

CREATE TABLE IF NOT EXISTS auditor_access_grants (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    auditor_email TEXT NOT NULL,
    secret_digest BYTEA NOT NULL UNIQUE,
    created_by_user_id UUID NOT NULL REFERENCES users(id),
    created_via_agent_connection_id UUID NOT NULL REFERENCES agent_connections(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    CHECK (auditor_email = lower(trim(auditor_email))),
    CHECK (position('@' IN auditor_email) > 1),
    CHECK (expires_at > created_at),
    CHECK (revoked_at IS NULL OR revoked_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_auditor_access_grants_workspace_created
    ON auditor_access_grants (workspace_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_auditor_access_grants_active_lookup
    ON auditor_access_grants (workspace_id, secret_digest)
    WHERE revoked_at IS NULL;
