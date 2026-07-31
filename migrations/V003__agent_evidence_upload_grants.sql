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
        CHECK (content_type <> '' AND content_type = btrim(content_type)),
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
            (completed_at IS NOT NULL AND document_id IS NOT NULL AND completed_at >= issued_at)
        )
);

CREATE INDEX agent_evidence_upload_grants_eligibility_idx
    ON agent_evidence_upload_grants (id, workspace_id, expires_at)
    WHERE completed_at IS NULL;
