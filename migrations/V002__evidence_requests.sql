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
