use crate::{
    authentication::paseto::{
        AgentPolicyDocumentUploadGrantClaims, AgentPolicyDocumentUploadGrantDecryptor,
        VerifiedAgentPolicyDocumentUploadGrantClaims,
    },
    domain::AgentPolicyDocumentUploadAuthority,
};

pub use crate::authentication::paseto::AGENT_POLICY_DOCUMENT_UPLOAD_GRANT_AUDIENCE;

pub use crate::application::commands::issue_agent_policy_document_upload_grant::{
    AgentPolicyDocumentUploadGrantError, IssuedAgentPolicyDocumentUploadGrant,
};

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
            claims.upload_id.into(),
            claims.workspace_id.into(),
            claims.policy_id.into(),
            claims.issued_by_user_id.into(),
            claims.issued_via_agent_connection_id.into(),
            claims.expires_at,
        ))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use secrecy::SecretString;
    use url::Url;
    use uuid::Uuid;

    use super::*;
    use crate::{
        authentication::paseto::{
            AgentPolicyDocumentUploadGrantEncryptor, RegisteredClaims, VerifiedPasetoToken,
        },
        config::{PasetoUploadGrantConfig, PasetoUploadGrantKey},
        domain::{AgentPolicyDocumentUploadDeclaration, AgentPolicyDocumentUploadGrant},
    };

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
            claims: AgentPolicyDocumentUploadGrantClaims::new(
                upload_id,
                Uuid::new_v4(),
                Uuid::new_v4(),
                user_id,
                Uuid::new_v4(),
            ),
        }
    }
}
