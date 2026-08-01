#[derive(Clone, Copy)]
pub(crate) enum AgentEvidenceUploadGrantResult {
    Issued,
    ValidationRejected,
    Unavailable,
    Failed,
}

impl AgentEvidenceUploadGrantResult {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Issued => "issued",
            Self::ValidationRejected => "validation_rejected",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum AgentEvidenceUploadAttemptResult {
    Created,
    Replayed,
    ConcurrencyLost,
    ValidationRejected,
    Unavailable,
    StreamFailed,
    StorageFailed,
    DatabaseFailed,
}

impl AgentEvidenceUploadAttemptResult {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Replayed => "replayed",
            Self::ConcurrencyLost => "concurrency_lost",
            Self::ValidationRejected => "validation_rejected",
            Self::Unavailable => "unavailable",
            Self::StreamFailed => "stream_failed",
            Self::StorageFailed => "storage_failed",
            Self::DatabaseFailed => "database_failed",
        }
    }
}

pub(crate) fn record_grant(result: AgentEvidenceUploadGrantResult) {
    metrics::counter!(
        "proofplane_agent_evidence_upload_grants_total",
        "result" => result.as_str()
    )
    .increment(1);
}

pub(crate) fn record_attempt(result: AgentEvidenceUploadAttemptResult) {
    metrics::counter!(
        "proofplane_agent_evidence_upload_attempts_total",
        "result" => result.as_str()
    )
    .increment(1);
}

pub(crate) fn record_received_bytes(received_bytes: u64) {
    metrics::counter!("proofplane_agent_evidence_upload_received_bytes_total")
        .increment(received_bytes);
}
