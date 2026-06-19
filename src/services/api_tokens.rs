use std::sync::Arc;

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    authentication::paseto::{ApiTokenSigner, RegisteredClaims},
    domain::{
        canonical_permissions, ApiTokenId, ApiTokenWithPermissions, CreateApiTokenPayload,
        DomainError, UserId, WorkspaceId, WorkspacePermission,
    },
    repository::Postgres,
};

const API_AUDIENCE: &str = "proofplane-api";

#[derive(Clone)]
pub struct ApiTokenService {
    repository: Arc<Postgres>,
    signer: ApiTokenSigner,
}

#[derive(Debug, Error)]
pub enum ApiTokenError {
    #[error("API token request is invalid")]
    Invalid(Vec<String>),

    #[error("API token not found")]
    NotFound,

    #[error("API token issuance failed")]
    Issue(#[source] crate::authentication::paseto::Error),

    #[error("repository error")]
    Repository(#[from] crate::repository::Error),
}

pub struct IssuedUserApiToken {
    pub token: ApiTokenWithPermissions,
    pub raw_token: SecretString,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserApiTokenClaims {
    pub version: u8,
    pub workspace_id: Uuid,
    pub permissions: Vec<String>,
}

impl ApiTokenService {
    pub fn new(repository: Arc<Postgres>, signer: ApiTokenSigner) -> Self {
        Self { repository, signer }
    }

    pub async fn create_token(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
        name: String,
        expires_at: Option<DateTime<Utc>>,
        permissions: Vec<WorkspacePermission>,
    ) -> Result<IssuedUserApiToken, ApiTokenError> {
        self.authorize(workspace_id, user_id).await?;
        let expires_at = expires_at
            .ok_or_else(|| ApiTokenError::Invalid(vec!["expires_at is required".to_owned()]))?;
        if expires_at <= Utc::now() {
            return Err(ApiTokenError::Invalid(vec![
                "expires_at must be in the future".to_owned(),
            ]));
        }
        let permissions = canonical_permissions(permissions).map_err(domain_error)?;
        let token_id = ApiTokenId::from(Uuid::new_v4());
        let claims = UserApiTokenClaims {
            version: 1,
            workspace_id: Uuid::from(workspace_id),
            permissions: permissions
                .iter()
                .map(|permission| permission.as_str().to_owned())
                .collect(),
        };
        let issued = self
            .signer
            .issue(
                RegisteredClaims {
                    subject: Uuid::from(user_id),
                    token_id: Uuid::from(token_id),
                    expires_at,
                },
                &claims,
            )
            .map_err(ApiTokenError::Issue)?;
        let token = self
            .repository
            .create_api_token(&CreateApiTokenPayload {
                id: token_id,
                user_id,
                workspace_id,
                name,
                expires_at: issued.expires_at,
                permissions,
            })
            .await?;

        Ok(IssuedUserApiToken {
            token,
            raw_token: SecretString::from(issued.token),
        })
    }

    pub async fn list_tokens(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<ApiTokenWithPermissions>, ApiTokenError> {
        self.authorize(workspace_id, user_id).await?;

        Ok(self
            .repository
            .list_api_tokens_for_owner_workspace(user_id, workspace_id)
            .await?)
    }

    pub async fn revoke_token(
        &self,
        user_id: UserId,
        workspace_id: WorkspaceId,
        token_id: ApiTokenId,
    ) -> Result<(), ApiTokenError> {
        self.authorize(workspace_id, user_id).await?;
        let token = self
            .repository
            .get_api_token(token_id)
            .await?
            .filter(|token| {
                token.token.user_id == user_id && token.token.workspace_id == workspace_id
            })
            .ok_or(ApiTokenError::NotFound)?;

        if !self
            .repository
            .revoke_api_token_for_owner_workspace(token.token.id, user_id, workspace_id)
            .await?
        {
            return Err(ApiTokenError::NotFound);
        }

        Ok(())
    }

    async fn authorize(
        &self,
        workspace_id: WorkspaceId,
        user_id: UserId,
    ) -> Result<(), ApiTokenError> {
        self.repository
            .get_membership_role(workspace_id, user_id)
            .await?
            .ok_or(ApiTokenError::NotFound)?;

        Ok(())
    }
}

pub fn api_token_signer(
    public_api_base_url: url::Url,
    config: &crate::config::PasetoApiConfig,
) -> Result<ApiTokenSigner, crate::authentication::paseto::Error> {
    ApiTokenSigner::from_config(public_api_base_url, API_AUDIENCE, config)
}

fn domain_error(error: DomainError) -> ApiTokenError {
    ApiTokenError::Invalid(vec![error.to_string()])
}

#[cfg(test)]
mod tests {
    use chrono::Duration as ChronoDuration;
    use secrecy::SecretString;

    use super::*;
    use crate::{
        authentication::paseto::ApiTokenVerifier,
        config::{PasetoApiConfig, PasetoApiSigningKey, PasetoApiVerificationKey},
    };

    const API_SECRET: &str = "k4.secret.sEP9YtkNeO7EGJbpVYznvHnVXotZyGbkzuvHkOO3RgXAqGWIhrrfscm74zMx72tBOOD02gy8G4sB8-60b1cWiw";
    const API_PUBLIC: &str = "k4.public.wKhliIa637HJu-MzMe9rQTjg9NoMvBuLAfPutG9XFos";

    #[test]
    fn paseto_custom_claims_round_trip_with_expected_registered_claims() {
        let issuer = url::Url::parse("https://api.proofplane.test/").unwrap();
        let config = api_config();
        let signer = api_token_signer(issuer.clone(), &config).unwrap();
        let verifier = ApiTokenVerifier::from_config(issuer, API_AUDIENCE, &config).unwrap();
        let user_id = Uuid::new_v4();
        let token_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let expires_at = Utc::now() + ChronoDuration::days(90);

        let issued = signer
            .issue(
                RegisteredClaims {
                    subject: user_id,
                    token_id,
                    expires_at,
                },
                &UserApiTokenClaims {
                    version: 1,
                    workspace_id,
                    permissions: vec!["read_controls".to_owned(), "write_controls".to_owned()],
                },
            )
            .unwrap();
        let verified = verifier
            .verify::<UserApiTokenClaims>(&issued.token)
            .expect("token verifies");

        assert_eq!(issued.token_id, token_id);
        assert_eq!(verified.token_id, token_id);
        assert_eq!(verified.claims.version, 1);
        assert_eq!(verified.claims.workspace_id, workspace_id);
        assert_eq!(
            verified.claims.permissions,
            vec!["read_controls", "write_controls"]
        );
        assert_eq!(verified.expires_at, issued.expires_at);
    }

    fn api_config() -> PasetoApiConfig {
        PasetoApiConfig {
            active_signing_key: PasetoApiSigningKey {
                id: "local-api".to_owned(),
                secret: SecretString::from(API_SECRET),
            },
            verification_keys: vec![PasetoApiVerificationKey {
                id: "local-api".to_owned(),
                public: API_PUBLIC.to_owned(),
            }],
        }
    }
}
