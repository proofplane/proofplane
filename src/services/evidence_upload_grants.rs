use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    authentication::paseto::{
        RegisteredClaims, UploadGrantDecryptor, UploadGrantEncryptor, VerifiedPasetoToken,
    },
    domain::{
        AgentConnectionId, CoverageWindow, EvidenceId, EvidenceUploadGrantId, UserId, WorkspaceId,
    },
    repository::{NewEvidenceUploadGrant, Postgres},
};

use super::agent_connections::AgentConnectionContext;

const UPLOAD_GRANT_TOKEN_VERSION: u8 = 2;
const UPLOAD_GRANT_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadGrantClaims {
    version: u8,
    grant_id: String,
    workspace_id: String,
    evidence_id: String,
    valid_from: DateTime<Utc>,
    valid_until: DateTime<Utc>,
    issued_by_user_id: String,
    issued_via_agent_connection_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifiedUploadGrant {
    id: EvidenceUploadGrantId,
    workspace_id: WorkspaceId,
    evidence_id: EvidenceId,
    issued_by_user_id: UserId,
    issued_via: UploadGrantIssuer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedUploadGrant {
    pub url: Url,
    pub expires_at: DateTime<Utc>,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub audit: UploadGrantAuditContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedeemedUploadGrant {
    pub id: EvidenceUploadGrantId,
    pub workspace_id: WorkspaceId,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub issued_by_user_id: UserId,
    pub issued_via: UploadGrantIssuer,
    pub expires_at: DateTime<Utc>,
    pub redeemed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadGrantAuditContext {
    pub workspace_id: WorkspaceId,
    pub evidence_id: EvidenceId,
    pub issued_by_user_id: UserId,
    pub issued_via: UploadGrantIssuer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadGrantIssuer {
    AgentConnection(AgentConnectionId),
}

impl UploadGrantIssuer {
    pub fn agent_connection_id(self) -> AgentConnectionId {
        match self {
            Self::AgentConnection(id) => id,
        }
    }
}

#[derive(Clone)]
pub struct EvidenceUploadGrantService {
    repository: Arc<Postgres>,
    public_api_base_url: Url,
    grant_encryptor: UploadGrantEncryptor,
    grant_decryptor: UploadGrantDecryptor,
}

impl EvidenceUploadGrantService {
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
        connection: &AgentConnectionContext,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
    ) -> Result<IssuedUploadGrant, UploadGrantError> {
        self.issue_with_context(
            connection.workspace_id,
            connection.user_id,
            UploadGrantIssuer::AgentConnection(connection.connection_id),
            evidence_id,
            coverage,
        )
        .await
    }

    async fn issue_with_context(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
        issued_via: UploadGrantIssuer,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
    ) -> Result<IssuedUploadGrant, UploadGrantError> {
        let expires_at = Utc::now()
            + chrono::Duration::from_std(UPLOAD_GRANT_TTL)
                .map_err(|_| UploadGrantError::Internal)?;
        let grant_id = EvidenceUploadGrantId::from(Uuid::new_v4());
        let agent_connection_id = issued_via.agent_connection_id();
        let grant = self
            .repository
            .in_agent_connection_workspace_context(
                workspace_id,
                user_id,
                agent_connection_id,
                async move |context| {
                    context
                        .create_evidence_upload_grant(NewEvidenceUploadGrant {
                            id: grant_id,
                            evidence_id,
                            coverage,
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
                    subject: Uuid::from(user_id),
                    token_id: Uuid::from(grant.id),
                    expires_at: grant.expires_at,
                },
                &UploadGrantClaims {
                    version: UPLOAD_GRANT_TOKEN_VERSION,
                    grant_id: grant.id.to_string(),
                    workspace_id: grant.workspace_id.to_string(),
                    evidence_id: grant.evidence_id.to_string(),
                    valid_from: grant.coverage.valid_from,
                    valid_until: grant.coverage.valid_until,
                    issued_by_user_id: grant.issued_by_user_id.to_string(),
                    issued_via_agent_connection_id: grant
                        .issued_via_agent_connection_id
                        .map(|id| id.to_string()),
                },
            )
            .map_err(|_| UploadGrantError::Internal)?;
        let mut url = self
            .public_api_base_url
            .join("evidence-uploads")
            .map_err(|_| UploadGrantError::Internal)?;
        url.query_pairs_mut().append_pair("token", &issued.token);

        Ok(IssuedUploadGrant {
            url,
            expires_at: issued.expires_at,
            evidence_id,
            coverage: grant.coverage,
            audit: UploadGrantAuditContext {
                workspace_id,
                evidence_id,
                issued_by_user_id: user_id,
                issued_via,
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
            .redeem_evidence_upload_grant(grant.id, grant.workspace_id, grant.evidence_id)
            .await?
            .ok_or(UploadGrantError::Unavailable)?;
        let redeemed_at = redeemed.redeemed_at.ok_or(UploadGrantError::Internal)?;

        Ok(RedeemedUploadGrant {
            id: redeemed.id,
            workspace_id: redeemed.workspace_id,
            evidence_id: redeemed.evidence_id,
            coverage: redeemed.coverage,
            issued_by_user_id: redeemed.issued_by_user_id,
            issued_via: upload_grant_issuer_from_record(redeemed.issued_via_agent_connection_id)?,
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
        if claims.valid_until < claims.valid_from {
            return Err(InvalidUploadGrantClaims);
        }
        let id = EvidenceUploadGrantId::from(parse_uuid(&claims.grant_id)?);
        if id != EvidenceUploadGrantId::from(token_id) {
            return Err(InvalidUploadGrantClaims);
        }
        let issued_by_user_id = UserId::from(parse_uuid(&claims.issued_by_user_id)?);
        if issued_by_user_id != UserId::from(subject) {
            return Err(InvalidUploadGrantClaims);
        }

        Ok(Self {
            id,
            workspace_id: WorkspaceId::from(parse_uuid(&claims.workspace_id)?),
            evidence_id: EvidenceId::from(parse_uuid(&claims.evidence_id)?),
            issued_by_user_id,
            issued_via: upload_grant_issuer_from_claims(
                claims.issued_via_agent_connection_id.as_deref(),
            )?,
        })
    }
}

fn upload_grant_issuer_from_record(
    agent_connection_id: Option<AgentConnectionId>,
) -> Result<UploadGrantIssuer, UploadGrantError> {
    agent_connection_id
        .map(UploadGrantIssuer::AgentConnection)
        .ok_or(UploadGrantError::Internal)
}

fn upload_grant_issuer_from_claims(
    agent_connection_id: Option<&str>,
) -> Result<UploadGrantIssuer, InvalidUploadGrantClaims> {
    match agent_connection_id {
        Some(id) => Ok(UploadGrantIssuer::AgentConnection(AgentConnectionId::from(
            parse_uuid(id)?,
        ))),
        None => Err(InvalidUploadGrantClaims),
    }
}

#[derive(Debug)]
struct InvalidUploadGrantClaims;

#[derive(Debug, thiserror::Error)]
pub enum UploadGrantError {
    #[error("evidence upload grant is unavailable")]
    Unavailable,
    #[error("internal evidence upload grant error")]
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
            evidence_id: Uuid::new_v4().to_string(),
            valid_from: Utc::now() - chrono::Duration::days(90),
            valid_until: Utc::now(),
            issued_by_user_id: Uuid::new_v4().to_string(),
            issued_via_agent_connection_id: Some(Uuid::new_v4().to_string()),
        }
    }

    #[test]
    fn verified_upload_grant_accepts_valid_claims() {
        let claims = claims();
        let grant = VerifiedUploadGrant::try_from(verified_token(claims.clone())).unwrap();

        assert_eq!(grant.id.to_string(), claims.grant_id);
        assert_eq!(grant.workspace_id.to_string(), claims.workspace_id);
        assert_eq!(grant.evidence_id.to_string(), claims.evidence_id);
        assert_eq!(
            grant.issued_via.agent_connection_id().to_string(),
            claims.issued_via_agent_connection_id.unwrap()
        );
    }

    #[test]
    fn verified_upload_grant_rejects_wrong_version_and_bad_identifiers() {
        let mut wrong_version = claims();
        wrong_version.version = UPLOAD_GRANT_TOKEN_VERSION + 1;
        assert!(VerifiedUploadGrant::try_from(verified_token(wrong_version)).is_err());

        let mut superseded_version = claims();
        superseded_version.version = 1;
        assert!(VerifiedUploadGrant::try_from(verified_token(superseded_version)).is_err());

        for mutate in [
            |claims: &mut UploadGrantClaims| claims.grant_id = "bad".to_owned(),
            |claims: &mut UploadGrantClaims| claims.workspace_id = "bad".to_owned(),
            |claims: &mut UploadGrantClaims| claims.evidence_id = "bad".to_owned(),
            |claims: &mut UploadGrantClaims| {
                claims.issued_via_agent_connection_id = Some("bad".to_owned())
            },
            |claims: &mut UploadGrantClaims| claims.issued_via_agent_connection_id = None,
        ] {
            let mut claims = claims();
            mutate(&mut claims);
            assert!(VerifiedUploadGrant::try_from(verified_token(claims)).is_err());
        }
    }

    #[test]
    fn verified_upload_grant_rejects_inverted_coverage_window() {
        let mut claims = claims();
        std::mem::swap(&mut claims.valid_from, &mut claims.valid_until);

        assert!(VerifiedUploadGrant::try_from(verified_token(claims)).is_err());
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
