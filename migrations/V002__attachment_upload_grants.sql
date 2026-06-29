CREATE TABLE IF NOT EXISTS attachment_upload_grants (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    evidence_submission_id UUID NOT NULL REFERENCES evidence_submissions(id),
    issued_by_user_id UUID NOT NULL REFERENCES users(id),
    issued_via_api_token_id UUID NOT NULL REFERENCES api_tokens(id),
    issued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    redeemed_at TIMESTAMPTZ,
    CHECK (expires_at > issued_at),
    CHECK (redeemed_at IS NULL OR redeemed_at >= issued_at)
);

CREATE INDEX IF NOT EXISTS idx_attachment_upload_grants_redemption
    ON attachment_upload_grants (id, workspace_id, evidence_submission_id);

CREATE INDEX IF NOT EXISTS idx_attachment_upload_grants_expiry
    ON attachment_upload_grants (expires_at, redeemed_at);
