CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE workspaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug TEXT UNIQUE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_workspaces_created_id
    ON workspaces (created_at, id);

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    auth0_sub TEXT NOT NULL UNIQUE,
    email TEXT,
    name TEXT,
    last_login_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE workspace_memberships (
    user_id UUID NOT NULL REFERENCES users(id),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id)
);

CREATE INDEX idx_workspace_memberships_user_id
    ON workspace_memberships (user_id);

CREATE INDEX idx_workspace_memberships_workspace_role
    ON workspace_memberships (workspace_id, role);

CREATE TABLE outbox_messages (
    id BIGSERIAL PRIMARY KEY,
    topic TEXT NOT NULL,
    event_type TEXT NOT NULL,
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    payload JSONB NOT NULL,
    request_id UUID,
    message_kind TEXT NOT NULL CHECK (message_kind IN ('command', 'event')),
    message_type TEXT NOT NULL,
    message_version INTEGER NOT NULL CHECK (message_version >= 0),
    message_id UUID NOT NULL DEFAULT gen_random_uuid(),
    subject TEXT NOT NULL,
    correlation_id UUID,
    causation_id UUID,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_available_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_outbox_messages_due
    ON outbox_messages (next_available_at, id);

CREATE TABLE workspace_permissions (
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
    ('manage_auditor_access');

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

CREATE TABLE evidence (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    collection_instructions TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'retired')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_evidence_workspace_title
    ON evidence (workspace_id, title);

CREATE INDEX idx_evidence_active_workspace_title
    ON evidence (workspace_id, title)
    WHERE status = 'active';

CREATE TABLE frameworks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL
);

CREATE TABLE framework_requirements (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    framework_id UUID NOT NULL REFERENCES frameworks(id),
    code TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    UNIQUE (framework_id, code)
);

CREATE INDEX idx_framework_requirements_framework_code
    ON framework_requirements (framework_id, code);

CREATE TABLE controls (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    code TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, code)
);

CREATE INDEX idx_controls_workspace_code
    ON controls (workspace_id, code);

CREATE TABLE control_framework_requirement_mappings (
    control_id UUID NOT NULL REFERENCES controls(id),
    framework_requirement_id UUID NOT NULL REFERENCES framework_requirements(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (control_id, framework_requirement_id)
);

CREATE INDEX idx_control_requirement_mappings_requirement
    ON control_framework_requirement_mappings (framework_requirement_id, control_id);

CREATE TABLE evidence_control_mappings (
    evidence_id UUID NOT NULL REFERENCES evidence(id),
    control_id UUID NOT NULL REFERENCES controls(id),
    rationale TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (evidence_id, control_id)
);

CREATE INDEX idx_evidence_control_mappings_control
    ON evidence_control_mappings (control_id, evidence_id);

CREATE TABLE evidence_submissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    evidence_id UUID NOT NULL REFERENCES evidence(id),
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    valid_from TIMESTAMPTZ NOT NULL,
    valid_until TIMESTAMPTZ NOT NULL,
    submitted_by_agent_connection_id UUID NOT NULL REFERENCES agent_connections(id),
    CHECK (valid_until >= valid_from)
);

CREATE INDEX idx_evidence_submissions_evidence_received
    ON evidence_submissions (evidence_id, received_at DESC, id DESC);

CREATE TABLE documents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    owner_type TEXT NOT NULL
        CHECK (owner_type IN ('evidence_submission', 'policy')),
    owner_id UUID NOT NULL,
    created_by_user_id UUID NOT NULL REFERENCES users(id),
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    content_length BIGINT NOT NULL CHECK (content_length >= 0),
    object_key TEXT NOT NULL UNIQUE,
    checksum_sha256 TEXT NOT NULL,
    checksum_crc32c TEXT NOT NULL,
    archived BOOLEAN NOT NULL DEFAULT false,
    upload_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (upload_status IN ('pending', 'finalizing', 'uploaded', 'contains_virus', 'failed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX documents_owner_active_idx
    ON documents (workspace_id, owner_type, owner_id, filename, id)
    WHERE archived = false;

CREATE INDEX documents_upload_status_idx
    ON documents (upload_status, id)
    WHERE archived = false;

CREATE UNIQUE INDEX documents_policy_active_key
    ON documents (owner_id)
    WHERE owner_type = 'policy' AND archived = false;

CREATE UNIQUE INDEX documents_evidence_submission_key
    ON documents (owner_id)
    WHERE owner_type = 'evidence_submission';

CREATE TABLE document_upload_grants (
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
    CONSTRAINT document_upload_grants_coverage_window
        CHECK (valid_until >= valid_from),
    CHECK (expires_at > issued_at),
    CHECK (redeemed_at IS NULL OR redeemed_at >= issued_at)
);

CREATE INDEX idx_document_upload_grants_redemption
    ON document_upload_grants (id, workspace_id, evidence_id);

CREATE INDEX idx_document_upload_grants_expiry
    ON document_upload_grants (expires_at, redeemed_at);

CREATE TABLE policies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    name TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    archived_at TIMESTAMPTZ,
    CONSTRAINT policies_name_trimmed CHECK (name = btrim(name)),
    CONSTRAINT policies_name_length CHECK (char_length(name) BETWEEN 1 AND 200),
    CONSTRAINT policies_description_valid CHECK (
        description IS NULL
        OR (description = btrim(description) AND char_length(description) BETWEEN 1 AND 4000)
    )
);

CREATE UNIQUE INDEX policies_workspace_lower_name_active_key
    ON policies (workspace_id, lower(name))
    WHERE archived_at IS NULL;

CREATE INDEX idx_policies_workspace_active_name
    ON policies (workspace_id, lower(name), id)
    WHERE archived_at IS NULL;

CREATE TABLE policy_control_mappings (
    policy_id UUID NOT NULL REFERENCES policies(id),
    control_id UUID NOT NULL REFERENCES controls(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (policy_id, control_id)
);

CREATE INDEX idx_policy_control_mappings_control
    ON policy_control_mappings (control_id, policy_id);

CREATE TABLE auditor_access_grants (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    auditor_email TEXT NOT NULL,
    secret_digest BYTEA NOT NULL UNIQUE,
    created_by_user_id UUID NOT NULL REFERENCES users(id),
    created_via_agent_connection_id UUID NOT NULL REFERENCES agent_connections(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    period_start TIMESTAMPTZ NOT NULL,
    period_end TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    CHECK (auditor_email = lower(trim(auditor_email))),
    CHECK (position('@' IN auditor_email) > 1),
    CHECK (expires_at > created_at),
    CHECK (period_end >= period_start),
    CHECK (revoked_at IS NULL OR revoked_at >= created_at)
);

CREATE INDEX idx_auditor_access_grants_workspace_created
    ON auditor_access_grants (workspace_id, created_at DESC, id DESC);

CREATE INDEX idx_auditor_access_grants_active_lookup
    ON auditor_access_grants (workspace_id, secret_digest)
    WHERE revoked_at IS NULL;

CREATE TABLE auditor_sessions (
    id UUID PRIMARY KEY,
    grant_id UUID NOT NULL REFERENCES auditor_access_grants(id) ON DELETE CASCADE,
    session_digest BYTEA NOT NULL UNIQUE,
    auditor_email TEXT NOT NULL,
    auth0_subject TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (auditor_email = lower(trim(auditor_email))),
    CHECK (btrim(auth0_subject) <> ''),
    CHECK (octet_length(session_digest) = 32),
    CHECK (expires_at > created_at),
    CHECK (revoked_at IS NULL OR revoked_at >= created_at)
);

CREATE INDEX idx_auditor_sessions_active_lookup
    ON auditor_sessions (session_digest)
    WHERE revoked_at IS NULL;

CREATE INDEX idx_auditor_sessions_grant
    ON auditor_sessions (grant_id);

CREATE TABLE oauth_authorization_requests (
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

CREATE INDEX idx_oauth_authorization_requests_client
    ON oauth_authorization_requests (client_id, created_at DESC);

CREATE TABLE oauth_authorization_codes (
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

CREATE TABLE agent_evidence_upload_grants (
    id UUID PRIMARY KEY,
    submission_id UUID NOT NULL UNIQUE,
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    evidence_id UUID NOT NULL REFERENCES evidence(id),
    valid_from TIMESTAMPTZ NOT NULL,
    valid_until TIMESTAMPTZ NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    expected_content_length BIGINT NOT NULL,
    expected_sha256 BYTEA,
    issued_by_user_id UUID NOT NULL REFERENCES users(id),
    issued_via_agent_connection_id UUID NOT NULL REFERENCES agent_connections(id),
    issued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ,
    document_id UUID UNIQUE REFERENCES documents(id),
    CONSTRAINT agent_evidence_upload_grants_coverage_window
        CHECK (valid_until >= valid_from),
    CONSTRAINT agent_evidence_upload_grants_filename
        CHECK (filename <> '' AND octet_length(filename) <= 255),
    CONSTRAINT agent_evidence_upload_grants_content_type
        CHECK (
            content_type <> ''
            AND content_type = btrim(content_type)
            AND octet_length(content_type) <= 255
        ),
    CONSTRAINT agent_evidence_upload_grants_content_length
        CHECK (expected_content_length >= 0),
    CONSTRAINT agent_evidence_upload_grants_sha256
        CHECK (expected_sha256 IS NULL OR octet_length(expected_sha256) = 32),
    CONSTRAINT agent_evidence_upload_grants_expiry
        CHECK (expires_at > issued_at),
    CONSTRAINT agent_evidence_upload_grants_completion
        CHECK (
            (completed_at IS NULL AND document_id IS NULL)
            OR
            (
                completed_at IS NOT NULL
                AND document_id IS NOT NULL
                AND completed_at >= issued_at
                AND completed_at < expires_at
            )
        )
);

CREATE INDEX agent_evidence_upload_grants_eligibility_idx
    ON agent_evidence_upload_grants (id, workspace_id, expires_at)
    WHERE completed_at IS NULL;

CREATE TABLE agent_policy_document_upload_grants (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    policy_id UUID NOT NULL REFERENCES policies(id),
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    expected_content_length BIGINT NOT NULL,
    expected_sha256 BYTEA,
    issued_by_user_id UUID NOT NULL REFERENCES users(id),
    issued_via_agent_connection_id UUID NOT NULL REFERENCES agent_connections(id),
    issued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ,
    document_id UUID UNIQUE REFERENCES documents(id),
    CONSTRAINT agent_policy_document_upload_grants_filename
        CHECK (filename <> '' AND octet_length(filename) <= 255),
    CONSTRAINT agent_policy_document_upload_grants_content_type
        CHECK (
            content_type <> ''
            AND content_type = btrim(content_type)
            AND octet_length(content_type) <= 255
        ),
    CONSTRAINT agent_policy_document_upload_grants_content_length
        CHECK (expected_content_length >= 0),
    CONSTRAINT agent_policy_document_upload_grants_sha256
        CHECK (expected_sha256 IS NULL OR octet_length(expected_sha256) = 32),
    CONSTRAINT agent_policy_document_upload_grants_expiry
        CHECK (expires_at > issued_at),
    CONSTRAINT agent_policy_document_upload_grants_completion
        CHECK (
            (completed_at IS NULL AND document_id IS NULL)
            OR
            (
                completed_at IS NOT NULL
                AND document_id IS NOT NULL
                AND completed_at >= issued_at
                AND completed_at < expires_at
            )
        )
);

CREATE INDEX agent_policy_document_upload_grants_eligibility_idx
    ON agent_policy_document_upload_grants (id, workspace_id, expires_at)
    WHERE completed_at IS NULL;
