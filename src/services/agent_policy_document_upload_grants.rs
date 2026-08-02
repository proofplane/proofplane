use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::paseto::{
        AgentPolicyDocumentUploadGrantDecryptor, AgentPolicyDocumentUploadGrantEncryptor,
        RegisteredClaims, VerifiedPasetoToken,
    },
    domain::{
        AgentConnectionId, AgentPolicyDocumentUploadAuthority,
        AgentPolicyDocumentUploadDeclaration, AgentPolicyDocumentUploadGrant,
        AgentPolicyDocumentUploadGrantId, PolicyId, UserId, WorkspaceId,
    },
    repository::{PolicyDocumentUploadEligibility, Postgres},
};

use super::agent_connections::AgentConnectionContext;

const TOKEN_VERSION: u8 = 1;
const GRANT_TTL: Duration = Duration::from_secs(5 * 60);
pub const AGENT_POLICY_DOCUMENT_UPLOAD_GRANT_AUDIENCE: &str =
    "proofplane-agent-policy-document-upload-grant";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentPolicyDocumentUploadGrantClaims {
    version: u8,
    upload_id: String,
    workspace_id: String,
    policy_id: String,
    issued_by_user_id: String,
    issued_via_agent_connection_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifiedAgentPolicyDocumentUploadGrantClaims {
    upload_id: AgentPolicyDocumentUploadGrantId,
    workspace_id: WorkspaceId,
    policy_id: PolicyId,
    issued_by_user_id: UserId,
    issued_via_agent_connection_id: AgentConnectionId,
    expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AgentPolicyDocumentUploadGrantService {
    repository: Arc<Postgres>,
    encryptor: AgentPolicyDocumentUploadGrantEncryptor,
    credential_verifier: AgentPolicyDocumentUploadCredentialVerifier,
}

#[derive(Debug)]
pub struct IssuedAgentPolicyDocumentUploadGrant {
    pub grant: AgentPolicyDocumentUploadGrant,
    pub credential: SecretString,
}

#[derive(Clone)]
pub struct AgentPolicyDocumentUploadCredentialVerifier {
    decryptor: AgentPolicyDocumentUploadGrantDecryptor,
}

impl AgentPolicyDocumentUploadCredentialVerifier {
    pub fn new(decryptor: AgentPolicyDocumentUploadGrantDecryptor) -> Self {
        Self { decryptor }
    }

    pub fn verify(
        &self,
        credential: &str,
    ) -> Result<AgentPolicyDocumentUploadAuthority, AgentPolicyDocumentUploadGrantError> {
        let verified = self
            .decryptor
            .decrypt::<AgentPolicyDocumentUploadGrantClaims>(credential)
            .map_err(|_| AgentPolicyDocumentUploadGrantError::Unavailable)?;
        let claims = VerifiedAgentPolicyDocumentUploadGrantClaims::try_from(verified)
            .map_err(|_| AgentPolicyDocumentUploadGrantError::Unavailable)?;
        Ok(AgentPolicyDocumentUploadAuthority::new(
            claims.upload_id,
            claims.workspace_id,
            claims.policy_id,
            claims.issued_by_user_id,
            claims.issued_via_agent_connection_id,
            claims.expires_at,
        ))
    }
}

impl AgentPolicyDocumentUploadGrantService {
    pub fn new(
        repository: Arc<Postgres>,
        encryptor: AgentPolicyDocumentUploadGrantEncryptor,
        decryptor: AgentPolicyDocumentUploadGrantDecryptor,
    ) -> Self {
        Self {
            repository,
            encryptor,
            credential_verifier: AgentPolicyDocumentUploadCredentialVerifier::new(decryptor),
        }
    }

    pub fn credential_verifier(&self) -> AgentPolicyDocumentUploadCredentialVerifier {
        self.credential_verifier.clone()
    }

    pub async fn issue(
        &self,
        connection: &AgentConnectionContext,
        policy_id: PolicyId,
        declaration: AgentPolicyDocumentUploadDeclaration,
    ) -> Result<IssuedAgentPolicyDocumentUploadGrant, AgentPolicyDocumentUploadGrantError> {
        let upload_id = AgentPolicyDocumentUploadGrantId::from(Uuid::new_v4());
        let issued_at = Utc::now();
        let expires_at = issued_at
            + chrono::Duration::from_std(GRANT_TTL)
                .map_err(|_| AgentPolicyDocumentUploadGrantError::Internal)?;
        let issued = self
            .encryptor
            .encrypt(
                RegisteredClaims {
                    subject: Uuid::from(connection.user_id),
                    token_id: Uuid::from(upload_id),
                    expires_at,
                },
                &AgentPolicyDocumentUploadGrantClaims {
                    version: TOKEN_VERSION,
                    upload_id: upload_id.to_string(),
                    workspace_id: connection.workspace_id.to_string(),
                    policy_id: policy_id.to_string(),
                    issued_by_user_id: connection.user_id.to_string(),
                    issued_via_agent_connection_id: connection.connection_id.to_string(),
                },
            )
            .map_err(|_| AgentPolicyDocumentUploadGrantError::Internal)?;
        let grant = AgentPolicyDocumentUploadGrant::issue(
            upload_id,
            connection.workspace_id,
            policy_id,
            declaration,
            connection.user_id,
            connection.connection_id,
            issued_at,
            issued.expires_at,
        )
        .map_err(|_| AgentPolicyDocumentUploadGrantError::Internal)?;
        let grant = self
            .repository
            .in_agent_connection_workspace_context(
                connection.workspace_id,
                connection.user_id,
                connection.connection_id,
                async move |context| {
                    match context
                        .lock_policy_document_upload_eligibility(grant.policy_id())
                        .await?
                    {
                        None => return Ok(IssuePersistenceOutcome::Unavailable),
                        Some(PolicyDocumentUploadEligibility::CurrentDocument) => {
                            return Ok(IssuePersistenceOutcome::CurrentDocument)
                        }
                        Some(PolicyDocumentUploadEligibility::Eligible) => {}
                    }
                    let repository = context.agent_policy_document_upload_grants();
                    repository.save(&grant).await?;
                    Ok(
                        match repository.get(grant.id(), grant.workspace_id()).await? {
                            Some(grant) => IssuePersistenceOutcome::Issued(Box::new(grant)),
                            None => {
                                return Err(crate::repository::Error::InvariantViolation(
                                    "saved policy machine upload grant must be readable",
                                ))
                            }
                        },
                    )
                },
            )
            .await?;
        let grant = match grant {
            IssuePersistenceOutcome::Issued(grant) => *grant,
            IssuePersistenceOutcome::Unavailable => {
                return Err(AgentPolicyDocumentUploadGrantError::Unavailable)
            }
            IssuePersistenceOutcome::CurrentDocument => {
                return Err(AgentPolicyDocumentUploadGrantError::CurrentDocument)
            }
        };

        Ok(IssuedAgentPolicyDocumentUploadGrant {
            grant,
            credential: SecretString::from(issued.token),
        })
    }
}

enum IssuePersistenceOutcome {
    Issued(Box<AgentPolicyDocumentUploadGrant>),
    Unavailable,
    CurrentDocument,
}

impl TryFrom<VerifiedPasetoToken<AgentPolicyDocumentUploadGrantClaims>>
    for VerifiedAgentPolicyDocumentUploadGrantClaims
{
    type Error = InvalidAgentPolicyDocumentUploadGrantClaims;

    fn try_from(
        token: VerifiedPasetoToken<AgentPolicyDocumentUploadGrantClaims>,
    ) -> Result<Self, Self::Error> {
        if token.claims.version != TOKEN_VERSION {
            return Err(InvalidAgentPolicyDocumentUploadGrantClaims);
        }
        let upload_id =
            AgentPolicyDocumentUploadGrantId::from(parse_uuid(&token.claims.upload_id)?);
        let issued_by_user_id = UserId::from(parse_uuid(&token.claims.issued_by_user_id)?);
        if upload_id != AgentPolicyDocumentUploadGrantId::from(token.token_id)
            || issued_by_user_id != UserId::from(token.subject)
        {
            return Err(InvalidAgentPolicyDocumentUploadGrantClaims);
        }
        Ok(Self {
            upload_id,
            workspace_id: WorkspaceId::from(parse_uuid(&token.claims.workspace_id)?),
            policy_id: PolicyId::from(parse_uuid(&token.claims.policy_id)?),
            issued_by_user_id,
            issued_via_agent_connection_id: AgentConnectionId::from(parse_uuid(
                &token.claims.issued_via_agent_connection_id,
            )?),
            expires_at: token.expires_at,
        })
    }
}

#[derive(Debug)]
struct InvalidAgentPolicyDocumentUploadGrantClaims;

fn parse_uuid(value: &str) -> Result<Uuid, InvalidAgentPolicyDocumentUploadGrantClaims> {
    Uuid::parse_str(value).map_err(|_| InvalidAgentPolicyDocumentUploadGrantClaims)
}

#[derive(Debug, thiserror::Error)]
pub enum AgentPolicyDocumentUploadGrantError {
    #[error("agent policy document upload grant is unavailable")]
    Unavailable,
    #[error("policy already has a current document")]
    CurrentDocument,
    #[error("internal agent policy document upload grant error")]
    Internal,
    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use secrecy::SecretString;
    use url::Url;
    use uuid::Uuid;

    use super::*;
    use crate::config::{PasetoUploadGrantConfig, PasetoUploadGrantKey};

    #[test]
    fn policy_upload_claims_require_version_subject_token_id_and_typed_ids() {
        assert!(VerifiedAgentPolicyDocumentUploadGrantClaims::try_from(verified_token()).is_ok());

        let mut wrong_version = verified_token();
        wrong_version.claims.version += 1;
        assert!(VerifiedAgentPolicyDocumentUploadGrantClaims::try_from(wrong_version).is_err());

        let mut wrong_subject = verified_token();
        wrong_subject.subject = Uuid::new_v4();
        assert!(VerifiedAgentPolicyDocumentUploadGrantClaims::try_from(wrong_subject).is_err());

        let mut wrong_token_id = verified_token();
        wrong_token_id.token_id = Uuid::new_v4();
        assert!(VerifiedAgentPolicyDocumentUploadGrantClaims::try_from(wrong_token_id).is_err());

        let invalidators: [fn(&mut VerifiedPasetoToken<AgentPolicyDocumentUploadGrantClaims>); 5] = [
            |token: &mut VerifiedPasetoToken<AgentPolicyDocumentUploadGrantClaims>| {
                token.claims.upload_id = "invalid".to_owned()
            },
            |token: &mut VerifiedPasetoToken<AgentPolicyDocumentUploadGrantClaims>| {
                token.claims.workspace_id = "invalid".to_owned()
            },
            |token: &mut VerifiedPasetoToken<AgentPolicyDocumentUploadGrantClaims>| {
                token.claims.policy_id = "invalid".to_owned()
            },
            |token: &mut VerifiedPasetoToken<AgentPolicyDocumentUploadGrantClaims>| {
                token.claims.issued_by_user_id = "invalid".to_owned()
            },
            |token: &mut VerifiedPasetoToken<AgentPolicyDocumentUploadGrantClaims>| {
                token.claims.issued_via_agent_connection_id = "invalid".to_owned()
            },
        ];
        for invalid in invalidators {
            let mut token = verified_token();
            invalid(&mut token);
            assert!(VerifiedAgentPolicyDocumentUploadGrantClaims::try_from(token).is_err());
        }
    }

    #[test]
    fn policy_upload_verifier_rejects_an_expired_credential() {
        let config = PasetoUploadGrantConfig {
            active_key_id: "test-key".to_owned(),
            keys: vec![PasetoUploadGrantKey {
                id: "test-key".to_owned(),
                secret: SecretString::from("k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY"),
            }],
        };
        let issuer = Url::parse("https://proofplane.example/").unwrap();
        let encryptor = AgentPolicyDocumentUploadGrantEncryptor::from_config(
            issuer.clone(),
            AGENT_POLICY_DOCUMENT_UPLOAD_GRANT_AUDIENCE,
            &config,
        )
        .unwrap();
        let verifier = AgentPolicyDocumentUploadCredentialVerifier::new(
            AgentPolicyDocumentUploadGrantDecryptor::from_config(
                issuer,
                AGENT_POLICY_DOCUMENT_UPLOAD_GRANT_AUDIENCE,
                &config,
            )
            .unwrap(),
        );
        let token = verified_token();
        let credential = encryptor
            .encrypt_expired(
                RegisteredClaims {
                    subject: token.subject,
                    token_id: token.token_id,
                    expires_at: Utc::now() - Duration::seconds(1),
                },
                &token.claims,
            )
            .unwrap()
            .token;

        assert!(matches!(
            verifier.verify(&credential),
            Err(AgentPolicyDocumentUploadGrantError::Unavailable)
        ));
    }

    #[test]
    fn issued_policy_upload_grant_debug_redacts_credentials_and_declared_metadata() {
        let token = verified_token();
        let declaration = AgentPolicyDocumentUploadDeclaration::new(
            "secret-policy.pdf".to_owned(),
            "application/secret".to_owned(),
            42,
            None,
            1024,
        )
        .into_result()
        .unwrap();
        let grant = AgentPolicyDocumentUploadGrant::issue(
            token.claims.upload_id.parse::<Uuid>().unwrap().into(),
            token.claims.workspace_id.parse::<Uuid>().unwrap().into(),
            token.claims.policy_id.parse::<Uuid>().unwrap().into(),
            declaration,
            token
                .claims
                .issued_by_user_id
                .parse::<Uuid>()
                .unwrap()
                .into(),
            token
                .claims
                .issued_via_agent_connection_id
                .parse::<Uuid>()
                .unwrap()
                .into(),
            Utc::now(),
            Utc::now() + Duration::minutes(5),
        )
        .unwrap();
        let issued = IssuedAgentPolicyDocumentUploadGrant {
            grant,
            credential: SecretString::from("super-secret-credential"),
        };

        let debug = format!("{issued:?}");
        assert!(!debug.contains("super-secret-credential"));
        assert!(!debug.contains("secret-policy.pdf"));
        assert!(!debug.contains("application/secret"));
    }

    fn verified_token() -> VerifiedPasetoToken<AgentPolicyDocumentUploadGrantClaims> {
        let upload_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        VerifiedPasetoToken {
            subject: user_id,
            token_id: upload_id,
            key_id: "test-key".to_owned(),
            expires_at: Utc::now() + Duration::minutes(5),
            claims: AgentPolicyDocumentUploadGrantClaims {
                version: TOKEN_VERSION,
                upload_id: upload_id.to_string(),
                workspace_id: Uuid::new_v4().to_string(),
                policy_id: Uuid::new_v4().to_string(),
                issued_by_user_id: user_id.to_string(),
                issued_via_agent_connection_id: Uuid::new_v4().to_string(),
            },
        }
    }
}
