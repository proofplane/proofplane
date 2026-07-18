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

CREATE TABLE policy_documents (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    policy_id UUID NOT NULL REFERENCES policies(id),
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

CREATE UNIQUE INDEX policy_documents_policy_active_key
    ON policy_documents (policy_id)
    WHERE archived = false;

CREATE INDEX idx_policy_documents_upload_status
    ON policy_documents (upload_status, id)
    WHERE archived = false;
