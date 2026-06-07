use async_trait::async_trait;
use thiserror::Error;

use crate::object_storage::ObjectKey;

#[async_trait]
pub trait MalwareScanner {
    async fn scan_object(
        &self,
        request: ScanObjectRequest,
    ) -> Result<MalwareScanResult, MalwareScanError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanObjectRequest {
    pub object_key: ObjectKey,
    pub content_type: String,
    pub content_length: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalwareScanResult {
    pub scanner_name: String,
    pub scanner_version: Option<String>,
    pub outcome: MalwareScanOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MalwareScanOutcome {
    Clean,
    Malicious { reason: String },
    Failed { reason: String },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MalwareScanError {
    #[error("object was not found in storage")]
    ObjectNotFound,

    #[error("malware scanner is unavailable: {reason}")]
    Unavailable { reason: String },

    #[error("malware scanner timed out: {reason}")]
    Timeout { reason: String },

    #[error("invalid malware scan request: {reason}")]
    InvalidRequest { reason: String },

    #[error("internal malware scanner error: {reason}")]
    Internal { reason: String },
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopMalwareScanner;

#[async_trait]
impl MalwareScanner for NoopMalwareScanner {
    async fn scan_object(
        &self,
        _request: ScanObjectRequest,
    ) -> Result<MalwareScanResult, MalwareScanError> {
        Ok(MalwareScanResult {
            scanner_name: "noop".to_owned(),
            scanner_version: None,
            outcome: MalwareScanOutcome::Clean,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MalwareScanError, MalwareScanOutcome, MalwareScanResult, MalwareScanner,
        NoopMalwareScanner, ScanObjectRequest,
    };
    use uuid::Uuid;

    use crate::{domain::WorkspaceId, object_storage::ObjectKey};

    #[tokio::test]
    async fn noop_scan_object_returns_clean_with_scanner_metadata() {
        let request = scan_request();

        let result = NoopMalwareScanner
            .scan_object(request)
            .await
            .expect("noop scan succeeds");

        assert_eq!(
            result,
            MalwareScanResult {
                scanner_name: "noop".to_owned(),
                scanner_version: None,
                outcome: MalwareScanOutcome::Clean,
            }
        );
    }

    #[test]
    fn scan_object_request_accepts_object_key_and_known_metadata() {
        let object_key = ObjectKey::new(
            WorkspaceId::from(Uuid::new_v4()),
            "quarantine/evidence-submissions/submission/attachments/upload",
            "manual-qa.txt",
        )
        .expect("valid object key");

        let request = ScanObjectRequest {
            object_key: object_key.clone(),
            content_type: "text/plain".to_owned(),
            content_length: 42,
            sha256: "6d1b9f787dd2607c4a83d9c024f9fe1a0e95d1d5d45f100d4dfad4fda8f8720d".to_owned(),
        };

        assert_eq!(request.object_key, object_key);
        assert_eq!(request.content_type, "text/plain");
        assert_eq!(request.content_length, 42);
        assert_eq!(
            request.sha256,
            "6d1b9f787dd2607c4a83d9c024f9fe1a0e95d1d5d45f100d4dfad4fda8f8720d"
        );
    }

    #[test]
    fn malware_scan_outcome_supports_equality() {
        assert_eq!(MalwareScanOutcome::Clean, MalwareScanOutcome::Clean);
        assert_eq!(
            MalwareScanOutcome::Malicious {
                reason: "EICAR-Test-File".to_owned()
            },
            MalwareScanOutcome::Malicious {
                reason: "EICAR-Test-File".to_owned()
            }
        );
        assert_ne!(
            MalwareScanOutcome::Failed {
                reason: "scanner refused object".to_owned()
            },
            MalwareScanOutcome::Clean
        );
    }

    #[test]
    fn malware_scan_error_formats_basic_adapter_failures() {
        assert_eq!(
            MalwareScanError::ObjectNotFound.to_string(),
            "object was not found in storage"
        );
        assert_eq!(
            MalwareScanError::Unavailable {
                reason: "clamd connection refused".to_owned()
            }
            .to_string(),
            "malware scanner is unavailable: clamd connection refused"
        );
        assert_eq!(
            MalwareScanError::Timeout {
                reason: "scan exceeded 30s".to_owned()
            }
            .to_string(),
            "malware scanner timed out: scan exceeded 30s"
        );
        assert_eq!(
            MalwareScanError::InvalidRequest {
                reason: "missing object hash".to_owned()
            }
            .to_string(),
            "invalid malware scan request: missing object hash"
        );
        assert_eq!(
            MalwareScanError::Internal {
                reason: "adapter panic boundary".to_owned()
            }
            .to_string(),
            "internal malware scanner error: adapter panic boundary"
        );
    }

    fn scan_request() -> ScanObjectRequest {
        ScanObjectRequest {
            object_key: ObjectKey::new(
                WorkspaceId::from(Uuid::new_v4()),
                "quarantine/evidence-submissions/submission/attachments/upload",
                "manual-qa.txt",
            )
            .expect("valid object key"),
            content_type: "text/plain".to_owned(),
            content_length: 42,
            sha256: "6d1b9f787dd2607c4a83d9c024f9fe1a0e95d1d5d45f100d4dfad4fda8f8720d".to_owned(),
        }
    }
}
