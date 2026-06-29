use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    authentication::{
        paseto::{
            RegisteredClaims, UploadGrantDecryptor, UploadGrantEncryptor, VerifiedPasetoToken,
        },
        ApiTokenContext,
    },
    domain::{ApiTokenId, AttachmentUploadGrantId, EvidenceSubmissionId, UserId, WorkspaceId},
    repository::{NewAttachmentUploadGrant, Postgres},
};

const UPLOAD_GRANT_TOKEN_VERSION: u8 = 1;
const UPLOAD_GRANT_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadGrantClaims {
    version: u8,
    grant_id: String,
    workspace_id: String,
    submission_id: String,
    issued_by_user_id: String,
    issued_via_api_token_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifiedUploadGrant {
    id: AttachmentUploadGrantId,
    workspace_id: WorkspaceId,
    submission_id: EvidenceSubmissionId,
    issued_by_user_id: UserId,
    issued_via_api_token_id: ApiTokenId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedUploadGrant {
    pub url: Url,
    pub expires_at: DateTime<Utc>,
    pub submission_id: EvidenceSubmissionId,
    pub audit: UploadGrantAuditContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedeemedUploadGrant {
    pub id: AttachmentUploadGrantId,
    pub workspace_id: WorkspaceId,
    pub submission_id: EvidenceSubmissionId,
    pub issued_by_user_id: UserId,
    pub issued_via_api_token_id: ApiTokenId,
    pub expires_at: DateTime<Utc>,
    pub redeemed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadGrantAuditContext {
    pub workspace_id: WorkspaceId,
    pub submission_id: EvidenceSubmissionId,
    pub issued_by_user_id: UserId,
    pub issued_via_api_token_id: ApiTokenId,
}

#[derive(Clone)]
pub struct AttachmentUploadGrantService {
    repository: Arc<Postgres>,
    public_api_base_url: Url,
    grant_encryptor: UploadGrantEncryptor,
    grant_decryptor: UploadGrantDecryptor,
}

impl AttachmentUploadGrantService {
    pub fn new(
        repository: Arc<Postgres>,
        public_api_base_url: Url,
        grant_encryptor: UploadGrantEncryptor,
        grant_decryptor: UploadGrantDecryptor,
    ) -> Self {
        Self {
            repository,
            public_api_base_url,
            grant_encryptor,
            grant_decryptor,
        }
    }

    pub async fn issue(
        &self,
        token: &ApiTokenContext,
        submission_id: EvidenceSubmissionId,
    ) -> Result<IssuedUploadGrant, UploadGrantError> {
        let expires_at = Utc::now()
            + chrono::Duration::from_std(UPLOAD_GRANT_TTL)
                .map_err(|_| UploadGrantError::Internal)?;
        let grant_id = AttachmentUploadGrantId::from(Uuid::new_v4());
        let grant = self
            .repository
            .in_workspace_context(
                token.workspace_id,
                token.user_id,
                token.api_token_id,
                async move |context| {
                    context
                        .create_attachment_upload_grant(NewAttachmentUploadGrant {
                            id: grant_id,
                            evidence_submission_id: submission_id,
                            expires_at,
                        })
                        .await
                },
            )
            .await?
            .ok_or(UploadGrantError::Unavailable)?;

        let issued = self
            .grant_encryptor
            .encrypt(
                RegisteredClaims {
                    subject: Uuid::from(token.user_id),
                    token_id: Uuid::from(grant.id),
                    expires_at: grant.expires_at,
                },
                &UploadGrantClaims {
                    version: UPLOAD_GRANT_TOKEN_VERSION,
                    grant_id: grant.id.to_string(),
                    workspace_id: grant.workspace_id.to_string(),
                    submission_id: grant.evidence_submission_id.to_string(),
                    issued_by_user_id: grant.issued_by_user_id.to_string(),
                    issued_via_api_token_id: grant.issued_via_api_token_id.to_string(),
                },
            )
            .map_err(|_| UploadGrantError::Internal)?;
        let mut url = self
            .public_api_base_url
            .join("evidence-attachment-uploads")
            .map_err(|_| UploadGrantError::Internal)?;
        url.query_pairs_mut().append_pair("token", &issued.token);

        Ok(IssuedUploadGrant {
            url,
            expires_at: issued.expires_at,
            submission_id,
            audit: UploadGrantAuditContext {
                workspace_id: token.workspace_id,
                submission_id,
                issued_by_user_id: token.user_id,
                issued_via_api_token_id: token.api_token_id,
            },
        })
    }

    pub async fn redeem(&self, token: &str) -> Result<RedeemedUploadGrant, UploadGrantError> {
        let verified = self
            .grant_decryptor
            .decrypt::<UploadGrantClaims>(token)
            .map_err(|_| UploadGrantError::Unavailable)?;
        let grant =
            VerifiedUploadGrant::try_from(verified).map_err(|_| UploadGrantError::Unavailable)?;
        let redeemed = self
            .repository
            .redeem_attachment_upload_grant(grant.id, grant.workspace_id, grant.submission_id)
            .await?
            .ok_or(UploadGrantError::Unavailable)?;
        let redeemed_at = redeemed.redeemed_at.ok_or(UploadGrantError::Internal)?;

        Ok(RedeemedUploadGrant {
            id: redeemed.id,
            workspace_id: redeemed.workspace_id,
            submission_id: redeemed.evidence_submission_id,
            issued_by_user_id: redeemed.issued_by_user_id,
            issued_via_api_token_id: redeemed.issued_via_api_token_id,
            expires_at: redeemed.expires_at,
            redeemed_at,
        })
    }
}

impl TryFrom<VerifiedPasetoToken<UploadGrantClaims>> for VerifiedUploadGrant {
    type Error = InvalidUploadGrantClaims;

    fn try_from(token: VerifiedPasetoToken<UploadGrantClaims>) -> Result<Self, Self::Error> {
        let VerifiedPasetoToken {
            subject,
            token_id,
            key_id: _,
            expires_at: _,
            claims,
        } = token;
        if claims.version != UPLOAD_GRANT_TOKEN_VERSION {
            return Err(InvalidUploadGrantClaims);
        }
        let id = AttachmentUploadGrantId::from(parse_uuid(&claims.grant_id)?);
        if id != AttachmentUploadGrantId::from(token_id) {
            return Err(InvalidUploadGrantClaims);
        }
        let issued_by_user_id = UserId::from(parse_uuid(&claims.issued_by_user_id)?);
        if issued_by_user_id != UserId::from(subject) {
            return Err(InvalidUploadGrantClaims);
        }

        Ok(Self {
            id,
            workspace_id: WorkspaceId::from(parse_uuid(&claims.workspace_id)?),
            submission_id: EvidenceSubmissionId::from(parse_uuid(&claims.submission_id)?),
            issued_by_user_id,
            issued_via_api_token_id: ApiTokenId::from(parse_uuid(&claims.issued_via_api_token_id)?),
        })
    }
}

#[derive(Debug)]
struct InvalidUploadGrantClaims;

#[derive(Debug, thiserror::Error)]
pub enum UploadGrantError {
    #[error("attachment upload grant is unavailable")]
    Unavailable,
    #[error("internal attachment upload grant error")]
    Internal,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

fn parse_uuid(value: &str) -> Result<Uuid, InvalidUploadGrantClaims> {
    Uuid::parse_str(value).map_err(|_| InvalidUploadGrantClaims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verified_token(claims: UploadGrantClaims) -> VerifiedPasetoToken<UploadGrantClaims> {
        let subject = Uuid::new_v4();
        VerifiedPasetoToken {
            subject,
            token_id: Uuid::parse_str(&claims.grant_id).unwrap_or_else(|_| Uuid::new_v4()),
            key_id: "test-key".to_owned(),
            expires_at: Utc::now() + chrono::Duration::minutes(5),
            claims: UploadGrantClaims {
                issued_by_user_id: subject.to_string(),
                ..claims
            },
        }
    }

    fn claims() -> UploadGrantClaims {
        let grant_id = Uuid::new_v4();
        UploadGrantClaims {
            version: UPLOAD_GRANT_TOKEN_VERSION,
            grant_id: grant_id.to_string(),
            workspace_id: Uuid::new_v4().to_string(),
            submission_id: Uuid::new_v4().to_string(),
            issued_by_user_id: Uuid::new_v4().to_string(),
            issued_via_api_token_id: Uuid::new_v4().to_string(),
        }
    }

    #[test]
    fn verified_upload_grant_accepts_valid_claims() {
        let claims = claims();
        let grant = VerifiedUploadGrant::try_from(verified_token(claims.clone())).unwrap();

        assert_eq!(grant.id.to_string(), claims.grant_id);
        assert_eq!(grant.workspace_id.to_string(), claims.workspace_id);
        assert_eq!(grant.submission_id.to_string(), claims.submission_id);
        assert_eq!(
            grant.issued_via_api_token_id.to_string(),
            claims.issued_via_api_token_id
        );
    }

    #[test]
    fn verified_upload_grant_rejects_wrong_version_and_bad_identifiers() {
        let mut wrong_version = claims();
        wrong_version.version = UPLOAD_GRANT_TOKEN_VERSION + 1;
        assert!(VerifiedUploadGrant::try_from(verified_token(wrong_version)).is_err());

        for mutate in [
            |claims: &mut UploadGrantClaims| claims.grant_id = "bad".to_owned(),
            |claims: &mut UploadGrantClaims| claims.workspace_id = "bad".to_owned(),
            |claims: &mut UploadGrantClaims| claims.submission_id = "bad".to_owned(),
            |claims: &mut UploadGrantClaims| claims.issued_via_api_token_id = "bad".to_owned(),
        ] {
            let mut claims = claims();
            mutate(&mut claims);
            assert!(VerifiedUploadGrant::try_from(verified_token(claims)).is_err());
        }
    }

    #[test]
    fn verified_upload_grant_rejects_subject_or_token_id_mismatch() {
        let claims = claims();
        let mut wrong_token_id = verified_token(claims.clone());
        wrong_token_id.token_id = Uuid::new_v4();
        assert!(VerifiedUploadGrant::try_from(wrong_token_id).is_err());

        let mut wrong_subject = verified_token(claims);
        wrong_subject.subject = Uuid::new_v4();
        assert!(VerifiedUploadGrant::try_from(wrong_subject).is_err());
    }
}
