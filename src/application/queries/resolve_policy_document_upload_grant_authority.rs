use secrecy::{ExposeSecret, SecretString};

use crate::{
    application::ExecutionMetadata,
    authentication::paseto::{
        PolicyDocumentUploadGrantClaims, PolicyUploadGrantDecryptor,
        VerifiedPolicyDocumentUploadGrantClaims,
    },
    domain::PolicyDocumentUploadGrantAuthority,
};

pub struct ResolvePolicyDocumentUploadGrantAuthority {
    pub credential: SecretString,
}

#[derive(Clone)]
pub struct ResolvePolicyDocumentUploadGrantAuthorityHandler {
    decryptor: PolicyUploadGrantDecryptor,
}

impl ResolvePolicyDocumentUploadGrantAuthorityHandler {
    pub fn new(decryptor: PolicyUploadGrantDecryptor) -> Self {
        Self { decryptor }
    }

    pub async fn handle(
        &self,
        query: ResolvePolicyDocumentUploadGrantAuthority,
        _metadata: ExecutionMetadata,
    ) -> Result<PolicyDocumentUploadGrantAuthority, PolicyDocumentUploadGrantAuthorityError> {
        let verified = self
            .decryptor
            .decrypt::<PolicyDocumentUploadGrantClaims>(query.credential.expose_secret())
            .map_err(|_| PolicyDocumentUploadGrantAuthorityError::Unavailable)?;
        let claims = VerifiedPolicyDocumentUploadGrantClaims::try_from(verified)
            .map_err(|_| PolicyDocumentUploadGrantAuthorityError::Unavailable)?;
        Ok(PolicyDocumentUploadGrantAuthority::new(
            claims.grant_id.into(),
            claims.workspace_id.into(),
            claims.policy_id.into(),
            claims.issued_by_user_id.into(),
            claims.issued_via_agent_connection_id.into(),
            claims.expires_at,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PolicyDocumentUploadGrantAuthorityError {
    #[error("policy document upload grant is unavailable")]
    Unavailable,
}
