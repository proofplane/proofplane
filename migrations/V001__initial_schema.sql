CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS workspaces (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug TEXT UNIQUE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS actors (
    id TEXT PRIMARY KEY,
    actor_type TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS api_credentials (
    id TEXT PRIMARY KEY,
    actor_id TEXT NOT NULL UNIQUE REFERENCES actors(id),
    name TEXT NOT NULL,
    key_id TEXT NOT NULL,
    credential_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS audit_events (
    id BIGSERIAL PRIMARY KEY,
    workspace_id UUID REFERENCES workspaces(id),
    actor_id TEXT REFERENCES actors(id),
    event_type TEXT NOT NULL,
    event_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS outbox_messages (
    id BIGSERIAL PRIMARY KEY,
    topic TEXT NOT NULL,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    retry_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

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

CREATE INDEX IF NOT EXISTS idx_evidence_requests_workspace_id
    ON evidence_requests (workspace_id);

CREATE INDEX IF NOT EXISTS idx_evidence_requests_due_active
    ON evidence_requests (due_at)
    WHERE status = 'active';

-- Auth resolves the actor's single API credential from the claimed actor ID.
CREATE UNIQUE INDEX IF NOT EXISTS idx_api_credentials_actor_id
    ON api_credentials (actor_id);

-- Workspace lists are returned in creation order with ID as the stable tie-breaker.
CREATE INDEX IF NOT EXISTS idx_workspaces_created_id
    ON workspaces (created_at, id);

-- Workspace-scoped Evidence Request lists are ordered by due time and title.
-- The left prefix still supports workspace-only filtering.
DROP INDEX IF EXISTS idx_evidence_requests_workspace_id;
CREATE INDEX IF NOT EXISTS idx_evidence_requests_workspace_due_title
    ON evidence_requests (workspace_id, due_at, title);

-- Due reads constrain active requests by workspace and then scan due time in
-- response order.
DROP INDEX IF EXISTS idx_evidence_requests_due_active;
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
