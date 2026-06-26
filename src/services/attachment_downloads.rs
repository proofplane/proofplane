use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    authentication::{
        paseto::{
            DownloadGrantDecryptor, DownloadGrantEncryptor, RegisteredClaims, VerifiedPasetoToken,
        },
        ApiTokenContext,
    },
    domain::{
        ApiTokenId, AttachmentUploadStatus, EvidenceAttachment, EvidenceAttachmentId,
        EvidenceSubmissionId, UserId, WorkspaceId,
    },
    object_storage::{FilesystemObjectStore, ObjectKey, ObjectMetadata, ObjectStore, ObjectStream},
    repository::{AttachmentDownloadCandidate, Postgres},
};

const DOWNLOAD_TOKEN_VERSION: u8 = 1;
const DOWNLOAD_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DownloadClaims {
    version: u8,
    workspace_id: String,
    submission_id: String,
    attachment_id: String,
    issued_by_user_id: String,
    issued_via_api_token_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifiedDownloadGrant {
    workspace_id: WorkspaceId,
    submission_id: EvidenceSubmissionId,
    attachment_id: EvidenceAttachmentId,
    issued_by_user_id: UserId,
    issued_via_api_token_id: ApiTokenId,
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

pub struct DownloadedAttachment {
    pub attachment: EvidenceAttachment,
    pub object: ObjectStream,
    pub audit: DownloadGrantAuditContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadGrantAuditContext {
    pub workspace_id: WorkspaceId,
    pub submission_id: EvidenceSubmissionId,
    pub attachment_id: EvidenceAttachmentId,
    pub issued_by_user_id: UserId,
    pub issued_via_api_token_id: ApiTokenId,
}

#[derive(Clone)]
pub struct AttachmentDownloadService {
    repository: Arc<Postgres>,
    object_store: Arc<FilesystemObjectStore>,
    public_api_base_url: Url,
    grant_encryptor: DownloadGrantEncryptor,
    grant_decryptor: DownloadGrantDecryptor,
}

impl AttachmentDownloadService {
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
        token: &ApiTokenContext,
        submission_id: EvidenceSubmissionId,
        attachment_id: EvidenceAttachmentId,
    ) -> Result<IssuedDownloadGrant, DownloadError> {
        let candidate = self
            .repository
            .in_workspace_context_read(token.workspace_id, async move |context| {
                context
                    .get_attachment_for_download_grant(submission_id, attachment_id)
                    .await
            })
            .await?
            .ok_or(DownloadError::NotFound)?;

        match candidate.attachment.upload_status {
            AttachmentUploadStatus::PendingUpload | AttachmentUploadStatus::Finalizing => {
                return Err(DownloadError::NotReady)
            }
            AttachmentUploadStatus::ContainsVirus | AttachmentUploadStatus::FailedUpload => {
                return Err(DownloadError::NotFound)
            }
            AttachmentUploadStatus::Uploaded => {}
        }

        self.validate_metadata(&candidate).await?;

        let expires_at = Utc::now()
            + chrono::Duration::from_std(DOWNLOAD_TTL).map_err(|_| DownloadError::Internal)?;
        let issued = self
            .grant_encryptor
            .encrypt(
                RegisteredClaims {
                    subject: Uuid::from(token.user_id),
                    token_id: Uuid::new_v4(),
                    expires_at,
                },
                &DownloadClaims {
                    version: DOWNLOAD_TOKEN_VERSION,
                    workspace_id: token.workspace_id.to_string(),
                    submission_id: submission_id.to_string(),
                    attachment_id: candidate.attachment.id.to_string(),
                    issued_by_user_id: token.user_id.to_string(),
                    issued_via_api_token_id: token.api_token_id.to_string(),
                },
            )
            .map_err(|_| DownloadError::Internal)?;
        let mut url = self
            .public_api_base_url
            .join("attachment-downloads")
            .map_err(|_| DownloadError::Internal)?;
        url.query_pairs_mut().append_pair("token", &issued.token);

        Ok(IssuedDownloadGrant {
            url,
            expires_at: issued.expires_at,
            filename: candidate.attachment.filename,
            content_type: candidate.attachment.content_type,
            content_length: candidate.attachment.content_length,
            audit: DownloadGrantAuditContext {
                workspace_id: token.workspace_id,
                submission_id,
                attachment_id: candidate.attachment.id,
                issued_by_user_id: token.user_id,
                issued_via_api_token_id: token.api_token_id,
            },
        })
    }

    pub async fn redeem(&self, token: &str) -> Result<DownloadedAttachment, DownloadError> {
        let verified = self
            .grant_decryptor
            .decrypt::<DownloadClaims>(token)
            .map_err(|_| DownloadError::NotFound)?;

        let grant =
            VerifiedDownloadGrant::try_from(verified).map_err(|_| DownloadError::NotFound)?;

        let candidate = self
            .repository
            .in_workspace_context_read(grant.workspace_id, async move |context| {
                context
                    .get_attachment_for_download_grant(grant.submission_id, grant.attachment_id)
                    .await
            })
            .await?
            .ok_or(DownloadError::NotFound)?;

        match candidate.attachment.upload_status {
            AttachmentUploadStatus::Uploaded => {}
            _ => return Err(DownloadError::NotFound),
        }

        let key = finalized_object_key(&candidate)?;
        let object = self
            .object_store
            .get_object(&key)
            .await
            .map_err(storage_download_error)?;
        validate_metadata(&candidate.attachment, &object.metadata)?;

        Ok(DownloadedAttachment {
            attachment: candidate.attachment,
            object,
            audit: DownloadGrantAuditContext {
                workspace_id: grant.workspace_id,
                submission_id: grant.submission_id,
                attachment_id: grant.attachment_id,
                issued_by_user_id: grant.issued_by_user_id,
                issued_via_api_token_id: grant.issued_via_api_token_id,
            },
        })
    }

    async fn validate_metadata(
        &self,
        candidate: &AttachmentDownloadCandidate,
    ) -> Result<(), DownloadError> {
        let key = finalized_object_key(candidate)?;
        let metadata = self
            .object_store
            .head_object(&key)
            .await
            .map_err(storage_download_error)?;
        validate_metadata(&candidate.attachment, &metadata)
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
            attachment_id: EvidenceAttachmentId::from(parse_uuid(&claims.attachment_id)?),
            issued_by_user_id,
            issued_via_api_token_id: ApiTokenId::from(parse_uuid(&claims.issued_via_api_token_id)?),
        })
    }
}

#[derive(Debug)]
struct InvalidDownloadClaims;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("attachment download is not found or no longer eligible")]
    NotFound,
    #[error("attachment is not ready for download")]
    NotReady,
    #[error("attachment metadata does not match object storage")]
    MetadataMismatch,
    #[error("internal attachment download error")]
    Internal,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

fn finalized_object_key(
    candidate: &AttachmentDownloadCandidate,
) -> Result<ObjectKey, DownloadError> {
    let expected = ObjectKey::new(
        candidate.workspace_id,
        format!(
            "evidence-submissions/{}/attachments/{}",
            candidate.attachment.evidence_submission_id, candidate.attachment.id
        ),
        &candidate.attachment.filename,
    )
    .map_err(|_| DownloadError::NotFound)?;
    if expected.as_str() != candidate.attachment.object_key {
        return Err(DownloadError::NotFound);
    }

    Ok(expected)
}

fn validate_metadata(
    attachment: &EvidenceAttachment,
    metadata: &ObjectMetadata,
) -> Result<(), DownloadError> {
    let expected_length =
        u64::try_from(attachment.content_length).map_err(|_| DownloadError::MetadataMismatch)?;
    if metadata.content_type != attachment.content_type
        || metadata.content_length != expected_length
        || metadata.sha256 != attachment.checksum_sha256
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
            attachment_id: "00000000-0000-4000-8000-000000000003".to_owned(),
            issued_by_user_id: "00000000-0000-4000-8000-000000000004".to_owned(),
            issued_via_api_token_id: "00000000-0000-4000-8000-000000000005".to_owned(),
        }
    }

    #[test]
    fn converts_supported_download_claims_to_domain_ids() {
        let claims = claims();
        let grant = VerifiedDownloadGrant::try_from(verified_token(claims.clone())).unwrap();

        assert_eq!(grant.workspace_id.to_string(), claims.workspace_id);
        assert_eq!(grant.submission_id.to_string(), claims.submission_id);
        assert_eq!(grant.attachment_id.to_string(), claims.attachment_id);
        assert_eq!(
            grant.issued_by_user_id.to_string(),
            claims.issued_by_user_id
        );
        assert_eq!(
            grant.issued_via_api_token_id.to_string(),
            claims.issued_via_api_token_id
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
            |claims: &mut DownloadClaims| claims.attachment_id = "invalid".to_owned(),
            |claims: &mut DownloadClaims| claims.issued_by_user_id = "invalid".to_owned(),
            |claims: &mut DownloadClaims| claims.issued_via_api_token_id = "invalid".to_owned(),
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
