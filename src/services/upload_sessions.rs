use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::{
        paseto::{
            RegisteredClaims, UploadSessionDecryptor, UploadSessionEncryptor, VerifiedPasetoToken,
        },
        ApiTokenContext,
    },
    domain::{
        ApiTokenId, EvidenceSubmissionId, UserId, WorkspaceId, WorkspacePermission,
        WorkspacePermissions,
    },
};

const UPLOAD_SESSION_TOKEN_VERSION: u8 = 1;
pub const UPLOAD_SESSION_TTL: Duration = Duration::from_secs(15 * 60);
pub const UPLOAD_SESSION_AUDIENCE: &str = "proofplane-attachment-upload-session";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UploadSessionClaims {
    version: u8,
    workspace_id: String,
    submission_id: String,
    issued_by_user_id: String,
    issued_via_api_token_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedUploadSession {
    pub workspace_id: WorkspaceId,
    pub submission_id: EvidenceSubmissionId,
    pub issued_by_user_id: UserId,
    pub issued_via_api_token_id: ApiTokenId,
    pub expires_at: DateTime<Utc>,
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

    pub fn issue(
        &self,
        workspace_id: WorkspaceId,
        submission_id: EvidenceSubmissionId,
        issued_by_user_id: UserId,
        issued_via_api_token_id: ApiTokenId,
    ) -> Result<String, UploadSessionError> {
        let expires_at = Utc::now()
            + chrono::Duration::from_std(UPLOAD_SESSION_TTL)
                .map_err(|_| UploadSessionError::Internal)?;
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
                    submission_id: submission_id.to_string(),
                    issued_by_user_id: issued_by_user_id.to_string(),
                    issued_via_api_token_id: issued_via_api_token_id.to_string(),
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

impl VerifiedUploadSession {
    pub fn api_token_context(self) -> ApiTokenContext {
        ApiTokenContext {
            user_id: self.issued_by_user_id,
            api_token_id: self.issued_via_api_token_id,
            workspace_id: self.workspace_id,
            permissions: WorkspacePermissions::from_iter(WorkspacePermission::ALL),
        }
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

        Ok(Self {
            workspace_id: WorkspaceId::from(parse_uuid(&claims.workspace_id)?),
            submission_id: EvidenceSubmissionId::from(parse_uuid(&claims.submission_id)?),
            issued_by_user_id,
            issued_via_api_token_id: ApiTokenId::from(parse_uuid(&claims.issued_via_api_token_id)?),
            expires_at,
        })
    }
}

#[derive(Debug)]
struct InvalidUploadSessionClaims;

#[derive(Debug, thiserror::Error)]
pub enum UploadSessionError {
    #[error("attachment upload session is unavailable")]
    Unavailable,
    #[error("internal attachment upload session error")]
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
        let submission_id = EvidenceSubmissionId::from(Uuid::new_v4());
        let user_id = UserId::from(Uuid::new_v4());
        let api_token_id = ApiTokenId::from(Uuid::new_v4());

        let token = service
            .issue(workspace_id, submission_id, user_id, api_token_id)
            .unwrap();
        let session = service.verify(&token).unwrap();

        assert_eq!(session.workspace_id, workspace_id);
        assert_eq!(session.submission_id, submission_id);
        assert_eq!(session.issued_by_user_id, user_id);
        assert_eq!(session.issued_via_api_token_id, api_token_id);
        assert!(session.expires_at > Utc::now());
    }

    #[test]
    fn upload_session_rejects_wrong_audience_and_implicit_assertion() {
        let config = config();
        let token = service()
            .issue(
                WorkspaceId::from(Uuid::new_v4()),
                EvidenceSubmissionId::from(Uuid::new_v4()),
                UserId::from(Uuid::new_v4()),
                ApiTokenId::from(Uuid::new_v4()),
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
                        submission_id: Uuid::new_v4().to_string(),
                        issued_by_user_id: Uuid::new_v4().to_string(),
                        issued_via_api_token_id: Uuid::new_v4().to_string(),
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
            submission_id: Uuid::new_v4().to_string(),
            issued_by_user_id: subject.to_string(),
            issued_via_api_token_id: Uuid::new_v4().to_string(),
        };

        let mut wrong_version = valid.clone();
        wrong_version.version += 1;
        assert!(VerifiedUploadSession::try_from(verified(subject, wrong_version)).is_err());

        for mutate in [
            |claims: &mut UploadSessionClaims| claims.workspace_id = "bad".to_owned(),
            |claims: &mut UploadSessionClaims| claims.submission_id = "bad".to_owned(),
            |claims: &mut UploadSessionClaims| claims.issued_by_user_id = "bad".to_owned(),
            |claims: &mut UploadSessionClaims| claims.issued_via_api_token_id = "bad".to_owned(),
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
                    submission_id: Uuid::new_v4().to_string(),
                    issued_by_user_id: Uuid::new_v4().to_string(),
                    issued_via_api_token_id: Uuid::new_v4().to_string(),
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
