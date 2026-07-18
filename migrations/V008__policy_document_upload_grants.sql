CREATE TABLE policy_document_upload_grants (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    policy_id UUID NOT NULL REFERENCES policies(id),
    issued_by_user_id UUID NOT NULL REFERENCES users(id),
    issued_via_agent_connection_id UUID NOT NULL REFERENCES agent_connections(id),
    issued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    redeemed_at TIMESTAMPTZ,
    CHECK (expires_at > issued_at),
    CHECK (redeemed_at IS NULL OR redeemed_at >= issued_at)
);

CREATE INDEX policy_document_upload_grants_redemption_idx
    ON policy_document_upload_grants (id, workspace_id, policy_id);

CREATE INDEX policy_document_upload_grants_expiry_idx
    ON policy_document_upload_grants (expires_at, redeemed_at);
