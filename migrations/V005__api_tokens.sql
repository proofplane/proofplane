CREATE TABLE api_tokens (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    name TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_api_tokens_owner_workspace_created
    ON api_tokens (user_id, workspace_id, created_at DESC, id DESC);

CREATE TABLE api_token_permissions (
    api_token_id UUID NOT NULL REFERENCES api_tokens(id) ON DELETE CASCADE,
    permission TEXT NOT NULL CHECK (permission IN (
        'read_evidence_requests', 'write_evidence_requests',
        'read_evidence_submissions', 'write_evidence_submissions',
        'read_controls', 'write_controls')),
    PRIMARY KEY (api_token_id, permission)
);
