use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    authentication::paseto::{
        DownloadGrantDecryptor, DownloadGrantEncryptor, RegisteredClaims, VerifiedPasetoToken,
    },
    domain::{
        AgentConnectionId, Document, DocumentId, DocumentOwner, DocumentUploadStatus,
        EvidenceSubmissionId, PolicyId, UserId, WorkspaceId,
    },
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectMetadata, ObjectStore, ObjectStream},
    repository::{DocumentDownloadCandidate, Postgres},
};

const DOWNLOAD_TOKEN_VERSION: u8 = 1;
const DOWNLOAD_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadClaims {
    version: u8,
    workspace_id: String,
    submission_id: String,
    document_id: String,
    issued_by_user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    issued_via_api_token_id: Option<String>,
    issued_via_agent_connection_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifiedDownloadGrant {
    workspace_id: WorkspaceId,
    submission_id: EvidenceSubmissionId,
    document_id: DocumentId,
    issued_by_user_id: UserId,
    issued_via: DownloadGrantIssuer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedDownloadGrant {
    pub url: Url,
    pub expires_at: DateTime<Utc>,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub audit: DownloadGrantAuditContext,
}

pub struct DownloadedDocument {
    pub document: Document,
    pub object: ObjectStream,
    pub audit: DownloadGrantAuditContext,
}

pub struct WorkspaceDownloadedDocument {
    pub document: Document,
    pub object: ObjectStream,
    pub audit: WorkspaceDownloadAuditContext,
}

pub struct DownloadedPolicyDocument {
    pub document: Document,
    pub object: ObjectStream,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadGrantAuditContext {
    pub workspace_id: WorkspaceId,
    pub submission_id: EvidenceSubmissionId,
    pub document_id: DocumentId,
    pub issued_by_user_id: UserId,
    pub issued_via: DownloadGrantIssuer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadGrantIssuer {
    AgentConnection(AgentConnectionId),
}

impl DownloadGrantIssuer {
    pub fn agent_connection_id(self) -> AgentConnectionId {
        match self {
            Self::AgentConnection(id) => id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceDownloadAuditContext {
    pub workspace_id: WorkspaceId,
    pub submission_id: EvidenceSubmissionId,
    pub document_id: DocumentId,
}

#[derive(Clone)]
pub struct DocumentDownloadService {
    repository: Arc<Postgres>,
    object_store: Arc<FilesystemObjectStore>,
    public_api_base_url: Url,
    grant_encryptor: DownloadGrantEncryptor,
    grant_decryptor: DownloadGrantDecryptor,
}

impl DocumentDownloadService {
    pub fn new(
        repository: Arc<Postgres>,
        object_store: Arc<FilesystemObjectStore>,
        public_api_base_url: Url,
        grant_encryptor: DownloadGrantEncryptor,
        grant_decryptor: DownloadGrantDecryptor,
    ) -> Self {
        Self {
            repository,
            object_store,
            public_api_base_url,
            grant_encryptor,
            grant_decryptor,
        }
    }

    pub async fn issue(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
        issued_via: DownloadGrantIssuer,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<IssuedDownloadGrant, DownloadError> {
        let candidate = self
            .repository
            .in_workspace_context_read(workspace_id, async move |context| {
                context
                    .get_document_for_download_grant(submission_id, document_id)
                    .await
            })
            .await?
            .ok_or(DownloadError::NotFound)?;

        match candidate.document.upload_status {
            DocumentUploadStatus::PendingUpload | DocumentUploadStatus::Finalizing => {
                return Err(DownloadError::NotReady)
            }
            DocumentUploadStatus::ContainsVirus | DocumentUploadStatus::FailedUpload => {
                return Err(DownloadError::NotFound)
            }
            DocumentUploadStatus::Uploaded => {}
        }

        self.validate_metadata(&candidate).await?;

        let expires_at = Utc::now()
            + chrono::Duration::from_std(DOWNLOAD_TTL).map_err(|_| DownloadError::Internal)?;
        let issued = self
            .grant_encryptor
            .encrypt(
                RegisteredClaims {
                    subject: Uuid::from(user_id),
                    token_id: Uuid::new_v4(),
                    expires_at,
                },
                &DownloadClaims {
                    version: DOWNLOAD_TOKEN_VERSION,
                    workspace_id: workspace_id.to_string(),
                    submission_id: submission_id.to_string(),
                    document_id: candidate.document.id().to_string(),
                    issued_by_user_id: user_id.to_string(),
                    issued_via_api_token_id: None,
                    issued_via_agent_connection_id: Some(
                        issued_via.agent_connection_id().to_string(),
                    ),
                },
            )
            .map_err(|_| DownloadError::Internal)?;
        let mut url = self
            .public_api_base_url
            .join("document-downloads")
            .map_err(|_| DownloadError::Internal)?;
        url.query_pairs_mut().append_pair("token", &issued.token);

        let document_id = candidate.document.id();
        Ok(IssuedDownloadGrant {
            url,
            expires_at: issued.expires_at,
            filename: candidate.document.filename,
            content_type: candidate.document.content_type,
            content_length: candidate.document.content_length,
            audit: DownloadGrantAuditContext {
                workspace_id,
                submission_id,
                document_id,
                issued_by_user_id: user_id,
                issued_via,
            },
        })
    }

    pub async fn redeem(&self, token: &str) -> Result<DownloadedDocument, DownloadError> {
        let verified = self
            .grant_decryptor
            .decrypt::<DownloadClaims>(token)
            .map_err(|_| DownloadError::NotFound)?;

        let grant =
            VerifiedDownloadGrant::try_from(verified).map_err(|_| DownloadError::NotFound)?;

        let downloaded = self
            .download_for_workspace(grant.workspace_id, grant.submission_id, grant.document_id)
            .await?;

        Ok(DownloadedDocument {
            document: downloaded.document,
            object: downloaded.object,
            audit: DownloadGrantAuditContext {
                workspace_id: grant.workspace_id,
                submission_id: grant.submission_id,
                document_id: grant.document_id,
                issued_by_user_id: grant.issued_by_user_id,
                issued_via: grant.issued_via,
            },
        })
    }

    pub async fn download_for_workspace(
        &self,
        workspace_id: WorkspaceId,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<WorkspaceDownloadedDocument, DownloadError> {
        let candidate = self
            .repository
            .in_workspace_context_read(workspace_id, async move |context| {
                context
                    .get_document_for_download_grant(submission_id, document_id)
                    .await
            })
            .await?
            .ok_or(DownloadError::NotFound)?;

        match candidate.document.upload_status {
            DocumentUploadStatus::Uploaded => {}
            _ => return Err(DownloadError::NotFound),
        }

        let key = finalized_object_key(&candidate)?;
        let object = self
            .object_store
            .get_object(&key)
            .await
            .map_err(storage_download_error)?;
        validate_metadata(&candidate.document, &object.metadata)?;

        Ok(WorkspaceDownloadedDocument {
            document: candidate.document,
            object,
            audit: WorkspaceDownloadAuditContext {
                workspace_id,
                submission_id,
                document_id,
            },
        })
    }

    pub async fn download_for_workspace_within_period(
        &self,
        workspace_id: WorkspaceId,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
        submission_id: EvidenceSubmissionId,
        document_id: DocumentId,
    ) -> Result<WorkspaceDownloadedDocument, DownloadError> {
        let candidate = self
            .repository
            .in_workspace_context_read(workspace_id, async move |context| {
                context
                    .get_document_for_download_grant_within_period(
                        submission_id,
                        document_id,
                        period_start,
                        period_end,
                    )
                    .await
            })
            .await?
            .ok_or(DownloadError::NotFound)?;

        match candidate.document.upload_status {
            DocumentUploadStatus::Uploaded => {}
            _ => return Err(DownloadError::NotFound),
        }

        let key = finalized_object_key(&candidate)?;
        let object = self
            .object_store
            .get_object(&key)
            .await
            .map_err(storage_download_error)?;
        validate_metadata(&candidate.document, &object.metadata)?;

        Ok(WorkspaceDownloadedDocument {
            document: candidate.document,
            object,
            audit: WorkspaceDownloadAuditContext {
                workspace_id,
                submission_id,
                document_id,
            },
        })
    }

    pub async fn download_policy_for_workspace(
        &self,
        workspace_id: WorkspaceId,
        policy_id: PolicyId,
        document_id: DocumentId,
    ) -> Result<DownloadedPolicyDocument, DownloadError> {
        let candidate = self
            .repository
            .in_workspace_context_read(workspace_id, async move |context| {
                context
                    .get_policy_document_for_download(policy_id, document_id)
                    .await
            })
            .await?
            .ok_or(DownloadError::NotFound)?;

        if candidate.document.upload_status != DocumentUploadStatus::Uploaded {
            return Err(DownloadError::NotFound);
        }

        let key = finalized_object_key(&candidate)?;
        let object = self
            .object_store
            .get_object(&key)
            .await
            .map_err(storage_download_error)?;
        validate_metadata(&candidate.document, &object.metadata)?;

        Ok(DownloadedPolicyDocument {
            document: candidate.document,
            object,
        })
    }

    async fn validate_metadata(
        &self,
        candidate: &DocumentDownloadCandidate,
    ) -> Result<(), DownloadError> {
        let key = finalized_object_key(candidate)?;
        let metadata = self
            .object_store
            .head_object(&key)
            .await
            .map_err(storage_download_error)?;
        validate_metadata(&candidate.document, &metadata)
    }
}

impl TryFrom<VerifiedPasetoToken<DownloadClaims>> for VerifiedDownloadGrant {
    type Error = InvalidDownloadClaims;

    fn try_from(token: VerifiedPasetoToken<DownloadClaims>) -> Result<Self, Self::Error> {
        let VerifiedPasetoToken {
            subject,
            token_id: _,
            key_id: _,
            claims,
            expires_at: _,
        } = token;
        if claims.version != DOWNLOAD_TOKEN_VERSION {
            return Err(InvalidDownloadClaims);
        }
        let issued_by_user_id = UserId::from(parse_uuid(&claims.issued_by_user_id)?);
        if issued_by_user_id != UserId::from(subject) {
            return Err(InvalidDownloadClaims);
        }

        Ok(Self {
            workspace_id: WorkspaceId::from(parse_uuid(&claims.workspace_id)?),
            submission_id: EvidenceSubmissionId::from(parse_uuid(&claims.submission_id)?),
            document_id: DocumentId::from(parse_uuid(&claims.document_id)?),
            issued_by_user_id,
            issued_via: download_grant_issuer_from_claims(
                claims.issued_via_api_token_id.as_deref(),
                claims.issued_via_agent_connection_id.as_deref(),
            )?,
        })
    }
}

fn download_grant_issuer_from_claims(
    api_token_id: Option<&str>,
    agent_connection_id: Option<&str>,
) -> Result<DownloadGrantIssuer, InvalidDownloadClaims> {
    match (api_token_id, agent_connection_id) {
        (None, Some(id)) => Ok(DownloadGrantIssuer::AgentConnection(
            AgentConnectionId::from(parse_uuid(id)?),
        )),
        _ => Err(InvalidDownloadClaims),
    }
}

#[derive(Debug)]
struct InvalidDownloadClaims;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("document download is not found or no longer eligible")]
    NotFound,
    #[error("document is not ready for download")]
    NotReady,
    #[error("document metadata does not match object storage")]
    MetadataMismatch,
    #[error("internal document download error")]
    Internal,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

fn finalized_object_key(candidate: &DocumentDownloadCandidate) -> Result<ObjectKey, DownloadError> {
    let owner_path = match candidate.document.owner() {
        DocumentOwner::EvidenceSubmission(id) => format!("evidence-submissions/{id}"),
        DocumentOwner::Policy(id) => format!("policies/{id}"),
    };
    let expected = ObjectKey::new(
        candidate.workspace_id,
        format!("{owner_path}/documents/{}", candidate.document.id()),
        &candidate.document.filename,
    )
    .map_err(|_| DownloadError::NotFound)?;
    if expected.as_str() != candidate.document.object_key {
        return Err(DownloadError::NotFound);
    }

    Ok(expected)
}

fn validate_metadata(document: &Document, metadata: &ObjectMetadata) -> Result<(), DownloadError> {
    let expected_length =
        u64::try_from(document.content_length).map_err(|_| DownloadError::MetadataMismatch)?;
    if metadata.content_type != document.content_type
        || metadata.content_length != expected_length
        || metadata.sha256 != document.checksum_sha256
    {
        return Err(DownloadError::MetadataMismatch);
    }

    Ok(())
}

fn storage_download_error(error: crate::object_storage::StorageError) -> DownloadError {
    match error {
        crate::object_storage::StorageError::NotFound => DownloadError::NotFound,
        _ => DownloadError::Internal,
    }
}

fn parse_uuid(value: &str) -> Result<Uuid, InvalidDownloadClaims> {
    Uuid::parse_str(value).map_err(|_| InvalidDownloadClaims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified_token(claims: DownloadClaims) -> VerifiedPasetoToken<DownloadClaims> {
        VerifiedPasetoToken {
            subject: Uuid::parse_str("00000000-0000-4000-8000-000000000004").unwrap(),
            token_id: Uuid::new_v4(),
            key_id: "download-key".to_owned(),
            claims,
            expires_at: Utc::now() + chrono::Duration::minutes(5),
        }
    }

    fn claims() -> DownloadClaims {
        DownloadClaims {
            version: DOWNLOAD_TOKEN_VERSION,
            workspace_id: "00000000-0000-4000-8000-000000000001".to_owned(),
            submission_id: "00000000-0000-4000-8000-000000000002".to_owned(),
            document_id: "00000000-0000-4000-8000-000000000003".to_owned(),
            issued_by_user_id: "00000000-0000-4000-8000-000000000004".to_owned(),
            issued_via_api_token_id: None,
            issued_via_agent_connection_id: Some("00000000-0000-4000-8000-000000000005".to_owned()),
        }
    }

    #[test]
    fn converts_supported_download_claims_to_domain_ids() {
        let claims = claims();
        let grant = VerifiedDownloadGrant::try_from(verified_token(claims.clone())).unwrap();

        assert_eq!(grant.workspace_id.to_string(), claims.workspace_id);
        assert_eq!(grant.submission_id.to_string(), claims.submission_id);
        assert_eq!(grant.document_id.to_string(), claims.document_id);
        assert_eq!(
            grant.issued_by_user_id.to_string(),
            claims.issued_by_user_id
        );
        assert_eq!(
            grant.issued_via.agent_connection_id().to_string(),
            claims.issued_via_agent_connection_id.unwrap()
        );
    }

    #[test]
    fn rejects_unsupported_download_claim_versions() {
        let mut claims = claims();
        claims.version += 1;

        assert!(VerifiedDownloadGrant::try_from(verified_token(claims)).is_err());
    }

    #[test]
    fn rejects_malformed_download_identity_claims() {
        for malformed in [
            |claims: &mut DownloadClaims| claims.workspace_id = "invalid".to_owned(),
            |claims: &mut DownloadClaims| claims.submission_id = "invalid".to_owned(),
            |claims: &mut DownloadClaims| claims.document_id = "invalid".to_owned(),
            |claims: &mut DownloadClaims| claims.issued_by_user_id = "invalid".to_owned(),
            |claims: &mut DownloadClaims| {
                claims.issued_via_agent_connection_id = Some("invalid".to_owned())
            },
        ] {
            let mut claims = claims();
            malformed(&mut claims);
            assert!(VerifiedDownloadGrant::try_from(verified_token(claims)).is_err());
        }
    }

    #[test]
    fn rejects_mismatched_subject_and_user_claim() {
        let mut verified = verified_token(claims());
        verified.subject = Uuid::new_v4();

        assert!(VerifiedDownloadGrant::try_from(verified).is_err());
    }
}
