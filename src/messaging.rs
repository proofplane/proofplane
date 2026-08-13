use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{DocumentIdentity, DocumentOwner};

pub const SCAN_DOCUMENT_TYPE: &str = "ScanDocument";
pub const FINALIZE_DOCUMENT_TYPE: &str = "FinalizeDocument";
pub const SEND_WORKSPACE_INVITATION_TYPE: &str = "SendWorkspaceInvitation";
pub const CURRENT_MESSAGE_VERSION: i32 = 1;
pub const LEGACY_DOCUMENT_SCAN_REQUESTED: &str = "document.scan_requested";
pub const LEGACY_DOCUMENT_FINALIZATION_REQUESTED: &str = "document.finalization_requested";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationMessageKind {
    Command,
    Event,
}

impl IntegrationMessageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Event => "event",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationMessageMetadata {
    pub message_id: Uuid,
    pub subject: String,
    pub correlation_id: Option<Uuid>,
    pub causation_id: Option<Uuid>,
}

impl IntegrationMessageMetadata {
    pub fn new(
        subject: impl Into<String>,
        correlation_id: Option<Uuid>,
        causation_id: Option<Uuid>,
    ) -> Self {
        Self {
            message_id: Uuid::new_v4(),
            subject: subject.into(),
            correlation_id,
            causation_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceDocumentWork {
    pub evidence_submission_id: Uuid,
    pub object_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDocumentWork {
    pub policy_id: Uuid,
    pub object_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DocumentWork {
    Evidence(EvidenceDocumentWork),
    Policy(PolicyDocumentWork),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceInvitationDeliveryWork {
    pub invitation_id: Uuid,
    pub generation: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum IntegrationPayload {
    Document(DocumentWork),
    WorkspaceInvitation(WorkspaceInvitationDeliveryWork),
}

impl IntegrationPayload {
    pub fn aggregate_type(&self) -> &'static str {
        match self {
            Self::Document(work) => work.aggregate_type(),
            Self::WorkspaceInvitation(_) => "workspace_invitation",
        }
    }
}

impl DocumentWork {
    pub fn new(identity: DocumentIdentity, object_key: impl Into<String>) -> Self {
        let object_key = object_key.into();
        match identity.owner() {
            DocumentOwner::EvidenceSubmission(id) => Self::Evidence(EvidenceDocumentWork {
                evidence_submission_id: id.into(),
                object_key,
            }),
            DocumentOwner::Policy(id) => Self::Policy(PolicyDocumentWork {
                policy_id: id.into(),
                object_key,
            }),
        }
    }

    pub fn aggregate_type(&self) -> &'static str {
        match self {
            Self::Evidence(_) => "evidence_document",
            Self::Policy(_) => "policy_document",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationMessage {
    ScanDocument {
        metadata: IntegrationMessageMetadata,
        payload: IntegrationPayload,
    },
    FinalizeDocument {
        metadata: IntegrationMessageMetadata,
        payload: IntegrationPayload,
    },
    SendWorkspaceInvitation {
        metadata: IntegrationMessageMetadata,
        payload: IntegrationPayload,
    },
}

impl IntegrationMessage {
    pub fn scan_document(
        identity: DocumentIdentity,
        object_key: impl Into<String>,
        correlation_id: Option<Uuid>,
        causation_id: Option<Uuid>,
    ) -> Self {
        Self::ScanDocument {
            metadata: IntegrationMessageMetadata::new(
                identity.document_uuid().to_string(),
                correlation_id,
                causation_id,
            ),
            payload: IntegrationPayload::Document(DocumentWork::new(identity, object_key)),
        }
    }

    pub fn finalize_document(
        identity: DocumentIdentity,
        object_key: impl Into<String>,
        correlation_id: Option<Uuid>,
        causation_id: Option<Uuid>,
    ) -> Self {
        Self::FinalizeDocument {
            metadata: IntegrationMessageMetadata::new(
                identity.document_uuid().to_string(),
                correlation_id,
                causation_id,
            ),
            payload: IntegrationPayload::Document(DocumentWork::new(identity, object_key)),
        }
    }

    pub fn send_workspace_invitation(
        invitation_id: Uuid,
        generation: i64,
        correlation_id: Option<Uuid>,
        causation_id: Option<Uuid>,
    ) -> Self {
        Self::SendWorkspaceInvitation {
            metadata: IntegrationMessageMetadata::new(
                invitation_id.to_string(),
                correlation_id,
                causation_id,
            ),
            payload: IntegrationPayload::WorkspaceInvitation(WorkspaceInvitationDeliveryWork {
                invitation_id,
                generation,
            }),
        }
    }

    pub fn metadata(&self) -> &IntegrationMessageMetadata {
        match self {
            Self::ScanDocument { metadata, .. }
            | Self::FinalizeDocument { metadata, .. }
            | Self::SendWorkspaceInvitation { metadata, .. } => metadata,
        }
    }

    pub fn kind(&self) -> IntegrationMessageKind {
        IntegrationMessageKind::Command
    }

    pub fn message_type(&self) -> &'static str {
        match self {
            Self::ScanDocument { .. } => SCAN_DOCUMENT_TYPE,
            Self::FinalizeDocument { .. } => FINALIZE_DOCUMENT_TYPE,
            Self::SendWorkspaceInvitation { .. } => SEND_WORKSPACE_INVITATION_TYPE,
        }
    }

    pub fn version(&self) -> i32 {
        CURRENT_MESSAGE_VERSION
    }

    pub fn payload(&self) -> &IntegrationPayload {
        match self {
            Self::ScanDocument { payload, .. }
            | Self::FinalizeDocument { payload, .. }
            | Self::SendWorkspaceInvitation { payload, .. } => payload,
        }
    }

    pub fn to_envelope(&self) -> Result<Value, serde_json::Error> {
        let metadata = self.metadata();
        serde_json::to_value(IntegrationEnvelopeRef {
            message_id: metadata.message_id,
            kind: self.kind(),
            message_type: self.message_type(),
            version: self.version(),
            subject: &metadata.subject,
            correlation_id: metadata.correlation_id,
            causation_id: metadata.causation_id,
            payload: self.payload(),
        })
    }

    pub fn from_envelope(value: Value) -> Result<Self, IntegrationMessageDecodeError> {
        let header: IntegrationEnvelopeHeader = serde_json::from_value(value.clone())
            .map_err(|_| IntegrationMessageDecodeError::MalformedEnvelope)?;
        if header.kind != IntegrationMessageKind::Command {
            return Err(IntegrationMessageDecodeError::UnknownKind(header.kind));
        }
        if header.version != CURRENT_MESSAGE_VERSION {
            return Err(IntegrationMessageDecodeError::UnsupportedVersion {
                message_type: header.message_type,
                version: header.version,
            });
        }

        let metadata = IntegrationMessageMetadata {
            message_id: header.message_id,
            subject: header.subject,
            correlation_id: header.correlation_id,
            causation_id: header.causation_id,
        };

        match header.message_type.as_str() {
            SCAN_DOCUMENT_TYPE => Ok(Self::ScanDocument {
                metadata,
                payload: IntegrationPayload::Document(
                    serde_json::from_value(header.payload)
                        .map_err(|_| IntegrationMessageDecodeError::MalformedPayload)?,
                ),
            }),
            FINALIZE_DOCUMENT_TYPE => Ok(Self::FinalizeDocument {
                metadata,
                payload: IntegrationPayload::Document(
                    serde_json::from_value(header.payload)
                        .map_err(|_| IntegrationMessageDecodeError::MalformedPayload)?,
                ),
            }),
            SEND_WORKSPACE_INVITATION_TYPE => Ok(Self::SendWorkspaceInvitation {
                metadata,
                payload: IntegrationPayload::WorkspaceInvitation(
                    serde_json::from_value(header.payload)
                        .map_err(|_| IntegrationMessageDecodeError::MalformedPayload)?,
                ),
            }),
            _ => Err(IntegrationMessageDecodeError::UnknownType(
                header.message_type,
            )),
        }
    }
}

#[derive(Serialize)]
struct IntegrationEnvelopeRef<'a> {
    message_id: Uuid,
    kind: IntegrationMessageKind,
    #[serde(rename = "type")]
    message_type: &'static str,
    version: i32,
    subject: &'a str,
    correlation_id: Option<Uuid>,
    causation_id: Option<Uuid>,
    payload: &'a IntegrationPayload,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntegrationEnvelopeHeader {
    message_id: Uuid,
    kind: IntegrationMessageKind,
    #[serde(rename = "type")]
    message_type: String,
    version: i32,
    subject: String,
    correlation_id: Option<Uuid>,
    causation_id: Option<Uuid>,
    payload: Value,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IntegrationMessageDecodeError {
    #[error("integration message envelope is malformed")]
    MalformedEnvelope,
    #[error("integration message kind {0:?} is not supported")]
    UnknownKind(IntegrationMessageKind),
    #[error("integration message type {0} is not supported")]
    UnknownType(String),
    #[error("integration message type {message_type} version {version} is not supported")]
    UnsupportedVersion { message_type: String, version: i32 },
    #[error("integration message payload is malformed")]
    MalformedPayload,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::{DocumentId, EvidenceSubmissionId};

    #[test]
    fn scan_document_v1_matches_the_golden_envelope() {
        let message = IntegrationMessage::ScanDocument {
            metadata: metadata(),
            payload: IntegrationPayload::Document(DocumentWork::Evidence(EvidenceDocumentWork {
                evidence_submission_id: uuid(2),
                object_key: "quarantine/document-1".to_owned(),
            })),
        };

        assert_eq!(
            message.to_envelope().expect("message serializes"),
            json!({
                "message_id": uuid(1),
                "kind": "command",
                "type": "ScanDocument",
                "version": 1,
                "subject": uuid(3).to_string(),
                "correlation_id": uuid(4),
                "causation_id": uuid(5),
                "payload": {
                    "evidence_submission_id": uuid(2),
                    "object_key": "quarantine/document-1"
                }
            })
        );
    }

    #[test]
    fn finalize_document_v1_round_trips_its_typed_payload() {
        let message = IntegrationMessage::FinalizeDocument {
            metadata: metadata(),
            payload: IntegrationPayload::Document(DocumentWork::Policy(PolicyDocumentWork {
                policy_id: uuid(6),
                object_key: "quarantine/policy-1".to_owned(),
            })),
        };

        let decoded =
            IntegrationMessage::from_envelope(message.to_envelope().expect("message serializes"))
                .expect("message decodes");

        assert_eq!(decoded, message);
    }

    #[test]
    fn workspace_invitation_delivery_v1_is_secret_free_and_round_trips() {
        let invitation_id = uuid(9);
        let message =
            IntegrationMessage::send_workspace_invitation(invitation_id, 3, Some(uuid(4)), None);
        let envelope = message.to_envelope().expect("message serializes");
        assert_eq!(
            envelope,
            json!({
                "message_id": message.metadata().message_id,
                "kind": "command",
                "type": "SendWorkspaceInvitation",
                "version": 1,
                "subject": invitation_id.to_string(),
                "correlation_id": uuid(4),
                "causation_id": null,
                "payload": {
                    "invitation_id": invitation_id,
                    "generation": 3
                }
            })
        );
        assert_eq!(
            IntegrationMessage::from_envelope(envelope).expect("message decodes"),
            message
        );
    }

    #[test]
    fn rejects_unknown_types_versions_kinds_and_malformed_payloads() {
        let base = IntegrationMessage::ScanDocument {
            metadata: metadata(),
            payload: IntegrationPayload::Document(DocumentWork::Evidence(EvidenceDocumentWork {
                evidence_submission_id: uuid(2),
                object_key: "quarantine/document-1".to_owned(),
            })),
        }
        .to_envelope()
        .expect("message serializes");

        let mut unknown_type = base.clone();
        unknown_type["type"] = json!("UnknownCommand");
        assert_eq!(
            IntegrationMessage::from_envelope(unknown_type),
            Err(IntegrationMessageDecodeError::UnknownType(
                "UnknownCommand".to_owned()
            ))
        );

        let mut unknown_version = base.clone();
        unknown_version["version"] = json!(2);
        assert_eq!(
            IntegrationMessage::from_envelope(unknown_version),
            Err(IntegrationMessageDecodeError::UnsupportedVersion {
                message_type: SCAN_DOCUMENT_TYPE.to_owned(),
                version: 2,
            })
        );

        let mut unknown_kind = base.clone();
        unknown_kind["kind"] = json!("event");
        assert_eq!(
            IntegrationMessage::from_envelope(unknown_kind),
            Err(IntegrationMessageDecodeError::UnknownKind(
                IntegrationMessageKind::Event
            ))
        );

        let mut malformed_payload = base;
        malformed_payload["payload"] = json!({ "object_key": 7 });
        assert_eq!(
            IntegrationMessage::from_envelope(malformed_payload),
            Err(IntegrationMessageDecodeError::MalformedPayload)
        );
    }

    #[test]
    fn constructors_derive_document_subject_and_owner_payload() {
        let document_id = DocumentId::from(uuid(3));
        let submission_id = EvidenceSubmissionId::from(uuid(2));
        let message = IntegrationMessage::scan_document(
            DocumentIdentity::Evidence {
                evidence_submission_id: submission_id,
                document_id,
            },
            "quarantine/document-1",
            Some(uuid(4)),
            None,
        );

        assert_eq!(message.metadata().subject, uuid(3).to_string());
        assert_eq!(message.payload().aggregate_type(), "evidence_document");
    }

    fn metadata() -> IntegrationMessageMetadata {
        IntegrationMessageMetadata {
            message_id: uuid(1),
            subject: uuid(3).to_string(),
            correlation_id: Some(uuid(4)),
            causation_id: Some(uuid(5)),
        }
    }

    fn uuid(value: u128) -> Uuid {
        Uuid::from_u128(value)
    }
}
