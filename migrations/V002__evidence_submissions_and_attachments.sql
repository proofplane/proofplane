CREATE TABLE IF NOT EXISTS evidence_submissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    evidence_request_id UUID NOT NULL REFERENCES evidence_requests(id),
    submitted_by UUID NOT NULL REFERENCES actors(id),
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    coverage_start_at TIMESTAMPTZ NOT NULL,
    coverage_end_at TIMESTAMPTZ NOT NULL,
    source_system TEXT NOT NULL,
    collection_method TEXT NOT NULL,
    provenance JSONB NOT NULL DEFAULT '{}'::jsonb,
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
    sha256 TEXT NOT NULL,
    caller_crc32c TEXT NOT NULL,
    scan_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (scan_status IN ('pending', 'clean', 'malicious', 'failed')),
    scanner_name TEXT,
    scanner_version TEXT,
    scanned_at TIMESTAMPTZ,
    scan_failure_reason TEXT
);

CREATE INDEX IF NOT EXISTS idx_evidence_attachments_submission
    ON evidence_attachments (evidence_submission_id, filename, id);
