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

INSERT INTO documents (
    id,
    workspace_id,
    owner_type,
    owner_id,
    created_by_user_id,
    filename,
    content_type,
    content_length,
    object_key,
    checksum_sha256,
    checksum_crc32c,
    archived,
    upload_status
)
SELECT
    a.id,
    er.workspace_id,
    'evidence_submission',
    a.evidence_submission_id,
    a.created_by_user_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.checksum_crc32c,
    a.archived,
    a.upload_status
FROM evidence_documents a
JOIN evidence_submissions s ON s.id = a.evidence_submission_id
JOIN evidence_requests er ON er.id = s.evidence_request_id;

INSERT INTO documents (
    id,
    workspace_id,
    owner_type,
    owner_id,
    created_by_user_id,
    filename,
    content_type,
    content_length,
    object_key,
    checksum_sha256,
    checksum_crc32c,
    archived,
    upload_status,
    created_at
)
SELECT
    a.id,
    p.workspace_id,
    'policy',
    a.policy_id,
    a.created_by_user_id,
    a.filename,
    a.content_type,
    a.content_length,
    a.object_key,
    a.checksum_sha256,
    a.checksum_crc32c,
    a.archived,
    a.upload_status,
    a.created_at
FROM policy_documents a
JOIN policies p ON p.id = a.policy_id;

CREATE INDEX documents_owner_active_idx
    ON documents (workspace_id, owner_type, owner_id, filename, id)
    WHERE archived = false;

CREATE INDEX documents_upload_status_idx
    ON documents (upload_status, id)
    WHERE archived = false;

CREATE UNIQUE INDEX documents_policy_active_key
    ON documents (owner_id)
    WHERE owner_type = 'policy' AND archived = false;

DROP TABLE policy_documents;
DROP TABLE evidence_documents;
