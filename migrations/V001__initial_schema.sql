CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS workspaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug TEXT UNIQUE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_workspaces_created_id
    ON workspaces (created_at, id);

CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    auth0_sub TEXT NOT NULL UNIQUE,
    email TEXT,
    name TEXT,
    last_login_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS workspace_memberships (
    user_id UUID NOT NULL REFERENCES users(id),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id)
);

CREATE INDEX IF NOT EXISTS idx_workspace_memberships_user_id
    ON workspace_memberships (user_id);

CREATE INDEX IF NOT EXISTS idx_workspace_memberships_workspace_role
    ON workspace_memberships (workspace_id, role);

CREATE TABLE IF NOT EXISTS outbox_messages (
    id BIGSERIAL PRIMARY KEY,
    topic TEXT NOT NULL,
    event_type TEXT NOT NULL,
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    payload JSONB NOT NULL,
    request_id UUID,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_available_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_outbox_messages_due
    ON outbox_messages (next_available_at, id);

CREATE TABLE IF NOT EXISTS workspace_permissions (
    permission TEXT PRIMARY KEY
);

INSERT INTO workspace_permissions (permission)
VALUES
    ('read_evidence'),
    ('write_evidence'),
    ('read_evidence_submissions'),
    ('write_evidence_submissions'),
    ('read_controls'),
    ('write_controls'),
    ('manage_auditor_access')
ON CONFLICT DO NOTHING;

CREATE TABLE IF NOT EXISTS agent_connections (
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

CREATE UNIQUE INDEX IF NOT EXISTS agent_connections_live_tuple_key
    ON agent_connections (user_id, auth0_client_id, resource)
    WHERE status IN ('pending', 'authorized', 'active');

CREATE TABLE IF NOT EXISTS evidence (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    collection_instructions TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'retired')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_evidence_workspace_title
    ON evidence (workspace_id, title);

CREATE INDEX IF NOT EXISTS idx_evidence_active_workspace_title
    ON evidence (workspace_id, title)
    WHERE status = 'active';

CREATE TABLE IF NOT EXISTS frameworks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS framework_requirements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    framework_id UUID NOT NULL REFERENCES frameworks(id),
    code TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    UNIQUE (framework_id, code)
);

CREATE INDEX IF NOT EXISTS idx_framework_requirements_framework_code
    ON framework_requirements (framework_id, code);

CREATE TABLE IF NOT EXISTS controls (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    code TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, code)
);

CREATE INDEX IF NOT EXISTS idx_controls_workspace_code
    ON controls (workspace_id, code);

CREATE TABLE IF NOT EXISTS control_framework_requirement_mappings (
    control_id UUID NOT NULL REFERENCES controls(id),
    framework_requirement_id UUID NOT NULL REFERENCES framework_requirements(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (control_id, framework_requirement_id)
);

CREATE INDEX IF NOT EXISTS idx_control_requirement_mappings_requirement
    ON control_framework_requirement_mappings (framework_requirement_id, control_id);

CREATE TABLE IF NOT EXISTS evidence_control_mappings (
    evidence_id UUID NOT NULL REFERENCES evidence(id),
    control_id UUID NOT NULL REFERENCES controls(id),
    rationale TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (evidence_id, control_id)
);

CREATE INDEX IF NOT EXISTS idx_evidence_control_mappings_control
    ON evidence_control_mappings (control_id, evidence_id);

CREATE TABLE IF NOT EXISTS evidence_submissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    evidence_id UUID NOT NULL REFERENCES evidence(id),
    submitted_by_agent_connection_id UUID NOT NULL REFERENCES agent_connections(id),
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_from TIMESTAMPTZ NOT NULL,
    valid_until TIMESTAMPTZ NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    content_length BIGINT NOT NULL CHECK (content_length >= 0),
    object_key TEXT NOT NULL UNIQUE,
    checksum_sha256 TEXT NOT NULL,
    checksum_crc32c TEXT NOT NULL,
    archived BOOLEAN NOT NULL DEFAULT false,
    upload_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (upload_status IN ('pending', 'finalizing', 'uploaded', 'contains_virus', 'failed')),
    CHECK (valid_until >= valid_from)
);

CREATE INDEX IF NOT EXISTS idx_evidence_submissions_evidence_received
    ON evidence_submissions (evidence_id, received_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_evidence_submissions_upload_status
    ON evidence_submissions (upload_status, id);

CREATE INDEX IF NOT EXISTS idx_evidence_submissions_coverage
    ON evidence_submissions (evidence_id, valid_from, valid_until, id)
    WHERE archived = false;

CREATE TABLE IF NOT EXISTS evidence_upload_grants (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    evidence_id UUID NOT NULL REFERENCES evidence(id),
    valid_from TIMESTAMPTZ NOT NULL,
    valid_until TIMESTAMPTZ NOT NULL,
    issued_by_user_id UUID NOT NULL REFERENCES users(id),
    issued_via_agent_connection_id UUID NOT NULL REFERENCES agent_connections(id),
    issued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    redeemed_at TIMESTAMPTZ,
    CHECK (valid_until >= valid_from),
    CHECK (expires_at > issued_at),
    CHECK (redeemed_at IS NULL OR redeemed_at >= issued_at)
);

CREATE INDEX IF NOT EXISTS idx_evidence_upload_grants_redemption
    ON evidence_upload_grants (id, workspace_id, evidence_id);

CREATE INDEX IF NOT EXISTS idx_evidence_upload_grants_expiry
    ON evidence_upload_grants (expires_at, redeemed_at);

CREATE INDEX IF NOT EXISTS idx_agent_connections_reusable
    ON agent_connections (auth0_subject, auth0_client_id, resource)
    WHERE status = 'active';

CREATE TABLE IF NOT EXISTS agent_connection_permissions (
    agent_connection_id UUID NOT NULL
        REFERENCES agent_connections(id) ON DELETE CASCADE,
    permission TEXT NOT NULL REFERENCES workspace_permissions(permission),
    PRIMARY KEY (agent_connection_id, permission)
);

CREATE TABLE IF NOT EXISTS agent_authorization_transactions (
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

CREATE TABLE IF NOT EXISTS oauth_authorization_requests (
    id UUID PRIMARY KEY,
    client_id TEXT NOT NULL,
    client_name TEXT NOT NULL,
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

CREATE TABLE IF NOT EXISTS oauth_authorization_codes (
    code_digest BYTEA PRIMARY KEY CHECK (octet_length(code_digest) = 32),
    request_id UUID NOT NULL UNIQUE REFERENCES oauth_authorization_requests(id) ON DELETE CASCADE,
    agent_connection_id UUID NOT NULL REFERENCES agent_connections(id),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    client_id TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    resource TEXT NOT NULL,
    scopes TEXT[] NOT NULL CHECK (cardinality(scopes) > 0),
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (expires_at > created_at)
);

CREATE INDEX IF NOT EXISTS idx_oauth_authorization_requests_client
    ON oauth_authorization_requests (client_id, created_at DESC);
