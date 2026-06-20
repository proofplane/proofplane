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
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS workspace_memberships (
    user_id UUID NOT NULL REFERENCES users(id),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, workspace_id)
);

CREATE INDEX IF NOT EXISTS idx_workspace_memberships_user_id
    ON workspace_memberships (user_id);

CREATE INDEX IF NOT EXISTS idx_workspace_memberships_workspace_role
    ON workspace_memberships (workspace_id, role);

CREATE TABLE IF NOT EXISTS api_tokens (
    id UUID PRIMARY KEY,
    digest BYTEA NOT NULL UNIQUE,
    user_id UUID NOT NULL REFERENCES users(id),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    name TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_api_tokens_owner_workspace_created
    ON api_tokens (user_id, workspace_id, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS api_token_permissions (
    api_token_id UUID NOT NULL REFERENCES api_tokens(id) ON DELETE CASCADE,
    permission TEXT NOT NULL CHECK (permission IN (
        'read_evidence_requests',
        'write_evidence_requests',
        'read_evidence_submissions',
        'write_evidence_submissions',
        'read_controls',
        'write_controls'
    )),
    PRIMARY KEY (api_token_id, permission)
);

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

CREATE TABLE IF NOT EXISTS evidence_requests (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    collection_instructions TEXT NOT NULL,
    cadence TEXT NOT NULL CHECK (cadence IN ('once', 'monthly', 'quarterly', 'annually')),
    due_at TIMESTAMPTZ NOT NULL,
    schedule_anchor_at TIMESTAMPTZ NOT NULL,
    freshness_window_days INTEGER CHECK (freshness_window_days IS NULL OR freshness_window_days > 0),
    status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'retired')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_evidence_requests_workspace_due_title
    ON evidence_requests (workspace_id, due_at, title);

CREATE INDEX IF NOT EXISTS idx_evidence_requests_active_workspace_due_title
    ON evidence_requests (workspace_id, due_at, title)
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

CREATE TABLE IF NOT EXISTS evidence_request_control_mappings (
    evidence_request_id UUID NOT NULL REFERENCES evidence_requests(id),
    control_id UUID NOT NULL REFERENCES controls(id),
    rationale TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (evidence_request_id, control_id)
);

CREATE INDEX IF NOT EXISTS idx_evidence_request_control_mappings_control
    ON evidence_request_control_mappings (control_id, evidence_request_id);

CREATE TABLE IF NOT EXISTS evidence_submissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    evidence_request_id UUID NOT NULL REFERENCES evidence_requests(id),
    submitted_by_api_token_id UUID NOT NULL REFERENCES api_tokens(id),
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    coverage_start_at TIMESTAMPTZ NOT NULL,
    coverage_end_at TIMESTAMPTZ NOT NULL,
    source_system TEXT NOT NULL,
    collection_method TEXT NOT NULL,
    CHECK (coverage_end_at >= coverage_start_at)
);

CREATE INDEX IF NOT EXISTS idx_evidence_submissions_request_received
    ON evidence_submissions (evidence_request_id, received_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS evidence_attachments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    evidence_submission_id UUID NOT NULL REFERENCES evidence_submissions(id),
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    content_length BIGINT NOT NULL CHECK (content_length >= 0),
    object_key TEXT NOT NULL UNIQUE,
    checksum_sha256 TEXT NOT NULL,
    checksum_crc32c TEXT NOT NULL,
    upload_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (upload_status IN ('pending', 'finalizing', 'uploaded', 'contains_virus', 'failed'))
);

CREATE INDEX IF NOT EXISTS idx_evidence_attachments_submission
    ON evidence_attachments (evidence_submission_id, filename, id);

CREATE INDEX IF NOT EXISTS idx_evidence_attachments_upload_status
    ON evidence_attachments (upload_status, id);
