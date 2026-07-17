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
        AgentConnectionId, EvidenceSubmission, EvidenceSubmissionId, SubmissionUploadStatus,
        UserId, WorkspaceId,
    },
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectMetadata, ObjectStore, ObjectStream},
    repository::{Postgres, SubmissionDownloadCandidate},
};

/// Bumped from 1 when submissions absorbed attachments and the grant stopped
/// naming a separate attachment. Old tokens must be rejected, not reinterpreted.
const DOWNLOAD_TOKEN_VERSION: u8 = 2;
const DOWNLOAD_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadClaims {
    version: u8,
    workspace_id: String,
    submission_id: String,
    issued_by_user_id: String,
    issued_via_agent_connection_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifiedDownloadGrant {
    workspace_id: WorkspaceId,
    submission_id: EvidenceSubmissionId,
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

pub struct DownloadedSubmission {
    pub submission: EvidenceSubmission,
    pub object: ObjectStream,
    pub audit: DownloadGrantAuditContext,
}

pub struct WorkspaceDownloadedSubmission {
    pub submission: EvidenceSubmission,
    pub object: ObjectStream,
    pub audit: WorkspaceDownloadAuditContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadGrantAuditContext {
    pub workspace_id: WorkspaceId,
    pub submission_id: EvidenceSubmissionId,
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
}

#[derive(Clone)]
pub struct SubmissionDownloadService {
    repository: Arc<Postgres>,
    object_store: Arc<FilesystemObjectStore>,
    public_api_base_url: Url,
    grant_encryptor: DownloadGrantEncryptor,
    grant_decryptor: DownloadGrantDecryptor,
}

impl SubmissionDownloadService {
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
    ) -> Result<IssuedDownloadGrant, DownloadError> {
        let candidate = self
            .repository
            .in_workspace_context_read(workspace_id, async move |context| {
                context
                    .get_submission_for_download_grant(submission_id)
                    .await
            })
            .await?
            .ok_or(DownloadError::NotFound)?;

        match candidate.submission.upload_status {
            SubmissionUploadStatus::PendingUpload | SubmissionUploadStatus::Finalizing => {
                return Err(DownloadError::NotReady)
            }
            SubmissionUploadStatus::ContainsVirus | SubmissionUploadStatus::FailedUpload => {
                return Err(DownloadError::NotFound)
            }
            SubmissionUploadStatus::Uploaded => {}
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
                    issued_by_user_id: user_id.to_string(),
                    issued_via_agent_connection_id: Some(
                        issued_via.agent_connection_id().to_string(),
                    ),
                },
            )
            .map_err(|_| DownloadError::Internal)?;
        let mut url = self
            .public_api_base_url
            .join("submission-downloads")
            .map_err(|_| DownloadError::Internal)?;
        url.query_pairs_mut().append_pair("token", &issued.token);

        Ok(IssuedDownloadGrant {
            url,
            expires_at: issued.expires_at,
            filename: candidate.submission.filename,
            content_type: candidate.submission.content_type,
            content_length: candidate.submission.content_length,
            audit: DownloadGrantAuditContext {
                workspace_id,
                submission_id,
                issued_by_user_id: user_id,
                issued_via,
            },
        })
    }

    pub async fn redeem(&self, token: &str) -> Result<DownloadedSubmission, DownloadError> {
        let verified = self
            .grant_decryptor
            .decrypt::<DownloadClaims>(token)
            .map_err(|_| DownloadError::NotFound)?;

        let grant =
            VerifiedDownloadGrant::try_from(verified).map_err(|_| DownloadError::NotFound)?;

        let downloaded = self
            .download_for_workspace(grant.workspace_id, grant.submission_id)
            .await?;

        Ok(DownloadedSubmission {
            submission: downloaded.submission,
            object: downloaded.object,
            audit: DownloadGrantAuditContext {
                workspace_id: grant.workspace_id,
                submission_id: grant.submission_id,
                issued_by_user_id: grant.issued_by_user_id,
                issued_via: grant.issued_via,
            },
        })
    }

    pub async fn download_for_workspace(
        &self,
        workspace_id: WorkspaceId,
        submission_id: EvidenceSubmissionId,
    ) -> Result<WorkspaceDownloadedSubmission, DownloadError> {
        let candidate = self
            .repository
            .in_workspace_context_read(workspace_id, async move |context| {
                context
                    .get_submission_for_download_grant(submission_id)
                    .await
            })
            .await?
            .ok_or(DownloadError::NotFound)?;

        match candidate.submission.upload_status {
            SubmissionUploadStatus::Uploaded => {}
            _ => return Err(DownloadError::NotFound),
        }

        let key = finalized_object_key(&candidate)?;
        let object = self
            .object_store
            .get_object(&key)
            .await
            .map_err(storage_download_error)?;
        validate_metadata(&candidate.submission, &object.metadata)?;

        Ok(WorkspaceDownloadedSubmission {
            submission: candidate.submission,
            object,
            audit: WorkspaceDownloadAuditContext {
                workspace_id,
                submission_id,
            },
        })
    }

    async fn validate_metadata(
        &self,
        candidate: &SubmissionDownloadCandidate,
    ) -> Result<(), DownloadError> {
        let key = finalized_object_key(candidate)?;
        let metadata = self
            .object_store
            .head_object(&key)
            .await
            .map_err(storage_download_error)?;
        validate_metadata(&candidate.submission, &metadata)
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
            issued_by_user_id,
            issued_via: download_grant_issuer_from_claims(
                claims.issued_via_agent_connection_id.as_deref(),
            )?,
        })
    }
}

fn download_grant_issuer_from_claims(
    agent_connection_id: Option<&str>,
) -> Result<DownloadGrantIssuer, InvalidDownloadClaims> {
    match agent_connection_id {
        Some(id) => Ok(DownloadGrantIssuer::AgentConnection(
            AgentConnectionId::from(parse_uuid(id)?),
        )),
        None => Err(InvalidDownloadClaims),
    }
}

#[derive(Debug)]
struct InvalidDownloadClaims;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("submission download is not found or no longer eligible")]
    NotFound,
    #[error("submission is not ready for download")]
    NotReady,
    #[error("submission metadata does not match object storage")]
    MetadataMismatch,
    #[error("internal submission download error")]
    Internal,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

fn finalized_object_key(
    candidate: &SubmissionDownloadCandidate,
) -> Result<ObjectKey, DownloadError> {
    let expected = ObjectKey::new(
        candidate.workspace_id,
        format!(
            "evidence/{}/submissions/{}",
            candidate.submission.evidence_id, candidate.submission.id
        ),
        &candidate.submission.filename,
    )
    .map_err(|_| DownloadError::NotFound)?;
    if expected.as_str() != candidate.submission.object_key {
        return Err(DownloadError::NotFound);
    }

    Ok(expected)
}

fn validate_metadata(
    submission: &EvidenceSubmission,
    metadata: &ObjectMetadata,
) -> Result<(), DownloadError> {
    let expected_length =
        u64::try_from(submission.content_length).map_err(|_| DownloadError::MetadataMismatch)?;
    if metadata.content_type != submission.content_type
        || metadata.content_length != expected_length
        || metadata.sha256 != submission.checksum_sha256
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
            issued_by_user_id: "00000000-0000-4000-8000-000000000004".to_owned(),
            issued_via_agent_connection_id: Some("00000000-0000-4000-8000-000000000005".to_owned()),
        }
    }

    #[test]
    fn converts_supported_download_claims_to_domain_ids() {
        let claims = claims();
        let grant = VerifiedDownloadGrant::try_from(verified_token(claims.clone())).unwrap();

        assert_eq!(grant.workspace_id.to_string(), claims.workspace_id);
        assert_eq!(grant.submission_id.to_string(), claims.submission_id);
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
    fn rejects_superseded_and_unknown_token_versions() {
        for version in [1, DOWNLOAD_TOKEN_VERSION + 1] {
            let mut claims = claims();
            claims.version = version;
            assert!(VerifiedDownloadGrant::try_from(verified_token(claims)).is_err());
        }
    }

    #[test]
    fn rejects_malformed_identifiers_and_missing_agent_connection() {
        for mutate in [
            |claims: &mut DownloadClaims| claims.workspace_id = "bad".to_owned(),
            |claims: &mut DownloadClaims| claims.submission_id = "bad".to_owned(),
            |claims: &mut DownloadClaims| {
                claims.issued_via_agent_connection_id = Some("bad".to_owned())
            },
            |claims: &mut DownloadClaims| claims.issued_via_agent_connection_id = None,
        ] {
            let mut claims = claims();
            mutate(&mut claims);
            assert!(VerifiedDownloadGrant::try_from(verified_token(claims)).is_err());
        }
    }

    #[test]
    fn rejects_subject_mismatch() {
        let mut claims = claims();
        claims.issued_by_user_id = "00000000-0000-4000-8000-000000000009".to_owned();

        assert!(VerifiedDownloadGrant::try_from(verified_token(claims)).is_err());
    }
}
