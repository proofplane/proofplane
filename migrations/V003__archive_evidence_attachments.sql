ALTER TABLE evidence_attachments
    ADD COLUMN archived BOOLEAN NOT NULL DEFAULT false;

CREATE INDEX IF NOT EXISTS idx_evidence_attachments_submission_active
    ON evidence_attachments (evidence_submission_id, filename, id)
    WHERE archived = false;
