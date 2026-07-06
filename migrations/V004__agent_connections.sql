CREATE TABLE workspace_permissions (
    permission TEXT PRIMARY KEY
);

INSERT INTO workspace_permissions (permission)
VALUES
    ('read_evidence_requests'),
    ('write_evidence_requests'),
    ('read_evidence_submissions'),
    ('write_evidence_submissions'),
    ('read_controls'),
    ('write_controls');

ALTER TABLE api_token_permissions
    DROP CONSTRAINT api_token_permissions_permission_check,
    ADD CONSTRAINT api_token_permissions_permission_fkey
        FOREIGN KEY (permission) REFERENCES workspace_permissions(permission);

CREATE TABLE agent_connections (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    auth0_subject TEXT NOT NULL,
    auth0_client_id TEXT NOT NULL,
    client_display_name TEXT NOT NULL,
    resource TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'active', 'revoked')),
    pending_expires_at TIMESTAMPTZ NOT NULL,
    activated_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT agent_connections_lifecycle CHECK (
        (status = 'pending'
            AND activated_at IS NULL
            AND revoked_at IS NULL)
        OR
        (status = 'active'
            AND activated_at IS NOT NULL
            AND revoked_at IS NULL)
        OR
        (status = 'revoked'
            AND revoked_at IS NOT NULL)
    ),
    CHECK (pending_expires_at > created_at),
    CHECK (activated_at IS NULL OR activated_at >= created_at),
    CHECK (last_used_at IS NULL OR last_used_at >= created_at),
    CHECK (revoked_at IS NULL OR revoked_at >= created_at)
);

CREATE UNIQUE INDEX agent_connections_live_tuple_key
    ON agent_connections (user_id, auth0_client_id, resource)
    WHERE status IN ('pending', 'active');

CREATE INDEX idx_agent_connections_reusable
    ON agent_connections (auth0_subject, auth0_client_id, resource)
    WHERE status = 'active';

CREATE TABLE agent_connection_permissions (
    agent_connection_id UUID NOT NULL
        REFERENCES agent_connections(id) ON DELETE CASCADE,
    permission TEXT NOT NULL REFERENCES workspace_permissions(permission),
    PRIMARY KEY (agent_connection_id, permission)
);

CREATE TABLE agent_authorization_transactions (
    id UUID PRIMARY KEY,
    agent_connection_id UUID NOT NULL UNIQUE
        REFERENCES agent_connections(id) ON DELETE CASCADE,
    continuation_digest BYTEA NOT NULL UNIQUE
        CHECK (octet_length(continuation_digest) = 32),
    nonce_digest BYTEA NOT NULL UNIQUE
        CHECK (octet_length(nonce_digest) = 32),
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (consumed_at IS NULL OR consumed_at >= created_at)
);
