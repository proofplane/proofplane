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
            (completed_at IS NOT NULL AND document_id IS NOT NULL AND completed_at >= issued_at)
        )
);

CREATE INDEX agent_policy_document_upload_grants_eligibility_idx
    ON agent_policy_document_upload_grants (id, workspace_id, expires_at)
    WHERE completed_at IS NULL;

ALTER TABLE agent_evidence_upload_grants
    DROP CONSTRAINT agent_evidence_upload_grants_content_type,
    ADD CONSTRAINT agent_evidence_upload_grants_content_type
        CHECK (
            content_type <> ''
            AND content_type = btrim(content_type)
            AND octet_length(content_type) <= 255
        );
