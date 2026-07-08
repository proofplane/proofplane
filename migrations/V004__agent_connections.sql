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

CREATE TABLE agent_connections (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    auth0_subject TEXT NOT NULL,
    auth0_client_id TEXT NOT NULL,
    client_display_name TEXT NOT NULL,
    resource TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'authorized', 'active', 'revoked')),
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
        (status = 'authorized'
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
    WHERE status IN ('pending', 'authorized', 'active');

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

ALTER TABLE evidence_submissions
    ADD COLUMN submitted_by_agent_connection_id UUID NOT NULL REFERENCES agent_connections(id);

ALTER TABLE attachment_upload_grants
    ADD COLUMN issued_via_agent_connection_id UUID NOT NULL REFERENCES agent_connections(id);

CREATE TABLE oauth_clients (
    id TEXT PRIMARY KEY,
    client_name TEXT NOT NULL,
    redirect_uris TEXT[] NOT NULL CHECK (cardinality(redirect_uris) > 0),
    token_endpoint_auth_method TEXT NOT NULL
        DEFAULT 'none'
        CHECK (token_endpoint_auth_method = 'none'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE oauth_authorization_requests (
    id UUID PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    state TEXT NOT NULL,
    resource TEXT NOT NULL,
    scopes TEXT[] NOT NULL CHECK (cardinality(scopes) > 0),
    auth0_subject TEXT,
    user_id UUID REFERENCES users(id),
    csrf_token_digest BYTEA NOT NULL UNIQUE CHECK (octet_length(csrf_token_digest) = 32),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    consumed_at TIMESTAMPTZ,
    CHECK (expires_at > created_at)
);

CREATE TABLE oauth_authorization_codes (
    code_digest BYTEA PRIMARY KEY CHECK (octet_length(code_digest) = 32),
    request_id UUID NOT NULL UNIQUE REFERENCES oauth_authorization_requests(id) ON DELETE CASCADE,
    agent_connection_id UUID NOT NULL REFERENCES agent_connections(id),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    client_id TEXT NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    resource TEXT NOT NULL,
    scopes TEXT[] NOT NULL CHECK (cardinality(scopes) > 0),
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (expires_at > created_at)
);

CREATE INDEX idx_oauth_authorization_requests_client
    ON oauth_authorization_requests (client_id, created_at DESC);
