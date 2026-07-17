use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::paseto::{
        RegisteredClaims, UploadSessionDecryptor, UploadSessionEncryptor, VerifiedPasetoToken,
    },
    domain::{AgentConnectionId, CoverageWindow, EvidenceId, UserId, WorkspaceId},
};

const UPLOAD_SESSION_TOKEN_VERSION: u8 = 2;
pub const UPLOAD_SESSION_TTL: Duration = Duration::from_secs(15 * 60);
pub const UPLOAD_SESSION_AUDIENCE: &str = "proofplane-evidence-upload-session";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadSessionClaims {
    version: u8,
    workspace_id: String,
    evidence_id: String,
    valid_from: DateTime<Utc>,
    valid_until: DateTime<Utc>,
    issued_by_user_id: String,
    issued_via_agent_connection_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedUploadSession {
    pub workspace_id: WorkspaceId,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub issued_by_user_id: UserId,
    pub issued_via: UploadSessionIssuer,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadSessionIssuer {
    AgentConnection(AgentConnectionId),
}

impl UploadSessionIssuer {
    pub fn agent_connection_id(self) -> AgentConnectionId {
        match self {
            Self::AgentConnection(id) => id,
        }
    }
}

#[derive(Clone)]
pub struct UploadSessionTokenService {
    encryptor: UploadSessionEncryptor,
    decryptor: UploadSessionDecryptor,
}

impl UploadSessionTokenService {
    pub fn new(encryptor: UploadSessionEncryptor, decryptor: UploadSessionDecryptor) -> Self {
        Self {
            encryptor,
            decryptor,
        }
    }

    pub fn issue_until(
        &self,
        workspace_id: WorkspaceId,
        evidence_id: EvidenceId,
        coverage: CoverageWindow,
        issued_by_user_id: UserId,
        issued_via: UploadSessionIssuer,
        expires_at: DateTime<Utc>,
    ) -> Result<String, UploadSessionError> {
        let issued = self
            .encryptor
            .encrypt(
                RegisteredClaims {
                    subject: Uuid::from(issued_by_user_id),
                    token_id: Uuid::new_v4(),
                    expires_at,
                },
                &UploadSessionClaims {
                    version: UPLOAD_SESSION_TOKEN_VERSION,
                    workspace_id: workspace_id.to_string(),
                    evidence_id: evidence_id.to_string(),
                    valid_from: coverage.valid_from,
                    valid_until: coverage.valid_until,
                    issued_by_user_id: issued_by_user_id.to_string(),
                    issued_via_agent_connection_id: Some(
                        issued_via.agent_connection_id().to_string(),
                    ),
                },
            )
            .map_err(|_| UploadSessionError::Internal)?;

        Ok(issued.token)
    }

    pub fn verify(&self, token: &str) -> Result<VerifiedUploadSession, UploadSessionError> {
        let verified = self
            .decryptor
            .decrypt::<UploadSessionClaims>(token)
            .map_err(|_| UploadSessionError::Unavailable)?;
        VerifiedUploadSession::try_from(verified).map_err(|_| UploadSessionError::Unavailable)
    }
}

impl TryFrom<VerifiedPasetoToken<UploadSessionClaims>> for VerifiedUploadSession {
    type Error = InvalidUploadSessionClaims;

    fn try_from(token: VerifiedPasetoToken<UploadSessionClaims>) -> Result<Self, Self::Error> {
        let VerifiedPasetoToken {
            subject,
            token_id: _,
            key_id: _,
            expires_at,
            claims,
        } = token;
        if claims.version != UPLOAD_SESSION_TOKEN_VERSION {
            return Err(InvalidUploadSessionClaims);
        }
        let issued_by_user_id = UserId::from(parse_uuid(&claims.issued_by_user_id)?);
        if issued_by_user_id != UserId::from(subject) {
            return Err(InvalidUploadSessionClaims);
        }
        let coverage = CoverageWindow::new(claims.valid_from, claims.valid_until)
            .map_err(|_| InvalidUploadSessionClaims)?;

        Ok(Self {
            workspace_id: WorkspaceId::from(parse_uuid(&claims.workspace_id)?),
            evidence_id: EvidenceId::from(parse_uuid(&claims.evidence_id)?),
            coverage,
            issued_by_user_id,
            issued_via: upload_session_issuer_from_claims(
                claims.issued_via_agent_connection_id.as_deref(),
            )?,
            expires_at,
        })
    }
}

fn upload_session_issuer_from_claims(
    agent_connection_id: Option<&str>,
) -> Result<UploadSessionIssuer, InvalidUploadSessionClaims> {
    match agent_connection_id {
        Some(id) => Ok(UploadSessionIssuer::AgentConnection(
            AgentConnectionId::from(parse_uuid(id)?),
        )),
        None => Err(InvalidUploadSessionClaims),
    }
}

#[derive(Debug)]
struct InvalidUploadSessionClaims;

#[derive(Debug, thiserror::Error)]
pub enum UploadSessionError {
    #[error("evidence upload session is unavailable")]
    Unavailable,
    #[error("internal evidence upload session error")]
    Internal,
}

fn parse_uuid(value: &str) -> Result<Uuid, InvalidUploadSessionClaims> {
    Uuid::parse_str(value).map_err(|_| InvalidUploadSessionClaims)
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use url::Url;

    use super::*;
    use crate::{
        authentication::paseto::{UploadGrantEncryptor, UploadSessionDecryptor},
        config::{PasetoUploadGrantConfig, PasetoUploadGrantKey},
    };

    const SECRET: &str = "k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY";

    fn config() -> PasetoUploadGrantConfig {
        PasetoUploadGrantConfig {
            active_key_id: "test-upload".to_owned(),
            keys: vec![PasetoUploadGrantKey {
                id: "test-upload".to_owned(),
                secret: SecretString::from(SECRET),
            }],
        }
    }

    fn issuer() -> Url {
        Url::parse("https://api.proofplane.test/").unwrap()
    }

    fn coverage() -> CoverageWindow {
        CoverageWindow::new(Utc::now() - chrono::Duration::days(90), Utc::now()).unwrap()
    }

    fn service() -> UploadSessionTokenService {
        let config = config();
        UploadSessionTokenService::new(
            UploadSessionEncryptor::from_config(issuer(), UPLOAD_SESSION_AUDIENCE, &config)
                .unwrap(),
            UploadSessionDecryptor::from_config(issuer(), UPLOAD_SESSION_AUDIENCE, &config)
                .unwrap(),
        )
    }

    #[test]
    fn upload_session_round_trip_succeeds_for_valid_claims() {
        let service = service();
        let workspace_id = WorkspaceId::from(Uuid::new_v4());
        let evidence_id = EvidenceId::from(Uuid::new_v4());
        let coverage = coverage();
        let user_id = UserId::from(Uuid::new_v4());
        let agent_connection_id = AgentConnectionId::from(Uuid::new_v4());

        let token = service
            .issue_until(
                workspace_id,
                evidence_id,
                coverage,
                user_id,
                UploadSessionIssuer::AgentConnection(agent_connection_id),
                Utc::now() + chrono::Duration::minutes(15),
            )
            .unwrap();
        let session = service.verify(&token).unwrap();

        assert_eq!(session.workspace_id, workspace_id);
        assert_eq!(session.evidence_id, evidence_id);
        assert_eq!(session.coverage, coverage);
        assert_eq!(session.issued_by_user_id, user_id);
        assert_eq!(
            session.issued_via,
            UploadSessionIssuer::AgentConnection(agent_connection_id)
        );
        assert!(session.expires_at > Utc::now());
    }

    #[test]
    fn upload_session_rejects_wrong_audience_and_implicit_assertion() {
        let config = config();
        let token = service()
            .issue_until(
                WorkspaceId::from(Uuid::new_v4()),
                EvidenceId::from(Uuid::new_v4()),
                coverage(),
                UserId::from(Uuid::new_v4()),
                UploadSessionIssuer::AgentConnection(AgentConnectionId::from(Uuid::new_v4())),
                Utc::now() + chrono::Duration::minutes(15),
            )
            .unwrap();

        let wrong_audience =
            UploadSessionDecryptor::from_config(issuer(), "wrong-audience", &config).unwrap();
        assert!(wrong_audience
            .decrypt::<UploadSessionClaims>(&token)
            .is_err());

        let grant_token =
            UploadGrantEncryptor::from_config(issuer(), UPLOAD_SESSION_AUDIENCE, &config)
                .unwrap()
                .encrypt(
                    RegisteredClaims {
                        subject: Uuid::new_v4(),
                        token_id: Uuid::new_v4(),
                        expires_at: Utc::now() + chrono::Duration::minutes(5),
                    },
                    &UploadSessionClaims {
                        version: UPLOAD_SESSION_TOKEN_VERSION,
                        workspace_id: Uuid::new_v4().to_string(),
                        evidence_id: Uuid::new_v4().to_string(),
                        valid_from: coverage().valid_from,
                        valid_until: coverage().valid_until,
                        issued_by_user_id: Uuid::new_v4().to_string(),
                        issued_via_agent_connection_id: Some(Uuid::new_v4().to_string()),
                    },
                )
                .unwrap();
        assert!(service().verify(&grant_token.token).is_err());
    }

    #[test]
    fn upload_session_rejects_bad_version_and_malformed_identifiers() {
        let subject = Uuid::new_v4();
        let valid = UploadSessionClaims {
            version: UPLOAD_SESSION_TOKEN_VERSION,
            workspace_id: Uuid::new_v4().to_string(),
            evidence_id: Uuid::new_v4().to_string(),
            valid_from: coverage().valid_from,
            valid_until: coverage().valid_until,
            issued_by_user_id: subject.to_string(),
            issued_via_agent_connection_id: Some(Uuid::new_v4().to_string()),
        };

        let mut wrong_version = valid.clone();
        wrong_version.version += 1;
        assert!(VerifiedUploadSession::try_from(verified(subject, wrong_version)).is_err());

        for mutate in [
            |claims: &mut UploadSessionClaims| claims.workspace_id = "bad".to_owned(),
            |claims: &mut UploadSessionClaims| claims.evidence_id = "bad".to_owned(),
            |claims: &mut UploadSessionClaims| claims.issued_by_user_id = "bad".to_owned(),
            |claims: &mut UploadSessionClaims| {
                claims.issued_via_agent_connection_id = Some("bad".to_owned())
            },
        ] {
            let mut claims = valid.clone();
            mutate(&mut claims);
            assert!(VerifiedUploadSession::try_from(verified(subject, claims)).is_err());
        }
    }

    #[test]
    fn upload_session_rejects_expired_and_malformed_tokens() {
        assert!(service().verify("not-a-token").is_err());

        let config = config();
        let token = UploadSessionEncryptor::from_config(issuer(), UPLOAD_SESSION_AUDIENCE, &config)
            .unwrap()
            .encrypt(
                RegisteredClaims {
                    subject: Uuid::new_v4(),
                    token_id: Uuid::new_v4(),
                    expires_at: Utc::now() + chrono::Duration::seconds(1),
                },
                &UploadSessionClaims {
                    version: UPLOAD_SESSION_TOKEN_VERSION,
                    workspace_id: Uuid::new_v4().to_string(),
                    evidence_id: Uuid::new_v4().to_string(),
                    valid_from: coverage().valid_from,
                    valid_until: coverage().valid_until,
                    issued_by_user_id: Uuid::new_v4().to_string(),
                    issued_via_agent_connection_id: Some(Uuid::new_v4().to_string()),
                },
            )
            .unwrap()
            .token;

        std::thread::sleep(std::time::Duration::from_secs(2));
        assert!(service().verify(&token).is_err());
    }

    fn verified(
        subject: Uuid,
        claims: UploadSessionClaims,
    ) -> VerifiedPasetoToken<UploadSessionClaims> {
        VerifiedPasetoToken {
            subject,
            token_id: Uuid::new_v4(),
            key_id: "test-upload".to_owned(),
            expires_at: Utc::now() + chrono::Duration::minutes(15),
            claims,
        }
    }
}
