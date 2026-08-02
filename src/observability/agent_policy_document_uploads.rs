#[derive(Clone, Copy)]
pub(crate) enum AgentPolicyDocumentUploadGrantResult {
    Issued,
    ValidationRejected,
    CurrentDocument,
    Unavailable,
    Failed,
}

impl AgentPolicyDocumentUploadGrantResult {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Issued => "issued",
            Self::ValidationRejected => "validation_rejected",
            Self::CurrentDocument => "current_document",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
        }
    }
}

pub(crate) fn record_grant(result: AgentPolicyDocumentUploadGrantResult) {
    metrics::counter!(
        "proofplane_agent_policy_document_upload_grants_total",
        "result" => result.as_str()
    )
    .increment(1);
}

#[derive(Clone, Copy)]
pub(crate) enum AgentPolicyDocumentUploadAttemptResult {
    Created,
    Replayed,
    ConcurrencyLost,
    CurrentDocument,
    ValidationRejected,
    Unavailable,
    StreamFailed,
    StorageFailed,
    DatabaseFailed,
}

impl AgentPolicyDocumentUploadAttemptResult {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Replayed => "replayed",
            Self::ConcurrencyLost => "concurrency_lost",
            Self::CurrentDocument => "current_document",
            Self::ValidationRejected => "validation_rejected",
            Self::Unavailable => "unavailable",
            Self::StreamFailed => "stream_failed",
            Self::StorageFailed => "storage_failed",
            Self::DatabaseFailed => "database_failed",
        }
    }
}

pub(crate) fn record_attempt(result: AgentPolicyDocumentUploadAttemptResult) {
    metrics::counter!(
        "proofplane_agent_policy_document_upload_attempts_total",
        "result" => result.as_str()
    )
    .increment(1);
}

pub(crate) fn record_received_bytes(received_bytes: u64) {
    metrics::counter!("proofplane_agent_policy_document_upload_received_bytes_total")
        .increment(received_bytes);
}

#[cfg(test)]
mod tests {
    use super::{
        record_attempt, record_grant, record_received_bytes,
        AgentPolicyDocumentUploadAttemptResult, AgentPolicyDocumentUploadGrantResult,
    };

    #[test]
    fn grant_metrics_use_only_the_bounded_result_taxonomy() {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let metrics = recorder.handle();
        let _guard = metrics::set_default_local_recorder(&recorder);

        for result in [
            AgentPolicyDocumentUploadGrantResult::Issued,
            AgentPolicyDocumentUploadGrantResult::ValidationRejected,
            AgentPolicyDocumentUploadGrantResult::CurrentDocument,
            AgentPolicyDocumentUploadGrantResult::Unavailable,
            AgentPolicyDocumentUploadGrantResult::Failed,
        ] {
            record_grant(result);
        }

        let rendered = metrics.render();
        for result in [
            "issued",
            "validation_rejected",
            "current_document",
            "unavailable",
            "failed",
        ] {
            assert!(rendered.contains(&format!(
                "proofplane_agent_policy_document_upload_grants_total{{result=\"{result}\"}} 1"
            )));
        }
    }

    #[test]
    fn attempt_metrics_use_only_the_bounded_result_taxonomy() {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let metrics = recorder.handle();
        let _guard = metrics::set_default_local_recorder(&recorder);

        for result in [
            AgentPolicyDocumentUploadAttemptResult::Created,
            AgentPolicyDocumentUploadAttemptResult::Replayed,
            AgentPolicyDocumentUploadAttemptResult::ConcurrencyLost,
            AgentPolicyDocumentUploadAttemptResult::CurrentDocument,
            AgentPolicyDocumentUploadAttemptResult::ValidationRejected,
            AgentPolicyDocumentUploadAttemptResult::Unavailable,
            AgentPolicyDocumentUploadAttemptResult::StreamFailed,
            AgentPolicyDocumentUploadAttemptResult::StorageFailed,
            AgentPolicyDocumentUploadAttemptResult::DatabaseFailed,
        ] {
            record_attempt(result);
        }

        let rendered = metrics.render();
        for result in [
            "created",
            "replayed",
            "concurrency_lost",
            "current_document",
            "validation_rejected",
            "unavailable",
            "stream_failed",
            "storage_failed",
            "database_failed",
        ] {
            assert!(rendered.contains(&format!(
                "proofplane_agent_policy_document_upload_attempts_total{{result=\"{result}\"}} 1"
            )));
        }
    }

    #[test]
    fn received_bytes_are_recorded_without_labels() {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let metrics = recorder.handle();
        let _guard = metrics::set_default_local_recorder(&recorder);

        record_received_bytes(483_920);

        assert!(metrics
            .render()
            .contains("proofplane_agent_policy_document_upload_received_bytes_total 483920"));
    }
}
