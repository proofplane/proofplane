use secrecy::{ExposeSecret, SecretString};

use crate::{
    application::ExecutionMetadata,
    authentication::paseto::{
        EvidenceDocumentUploadGrantClaims, UploadGrantDecryptor,
        VerifiedEvidenceDocumentUploadGrantClaims,
    },
    domain::{
        CoverageWindow, EvidenceDocumentUploadGrantAuthority, EvidenceDocumentUploadGrantError,
    },
};

pub struct ResolveEvidenceDocumentUploadGrantAuthority {
    pub credential: SecretString,
}

#[derive(Clone)]
pub struct ResolveEvidenceDocumentUploadGrantAuthorityHandler {
    decryptor: UploadGrantDecryptor,
}

impl ResolveEvidenceDocumentUploadGrantAuthorityHandler {
    pub fn new(decryptor: UploadGrantDecryptor) -> Self {
        Self { decryptor }
    }

    pub async fn handle(
        &self,
        query: ResolveEvidenceDocumentUploadGrantAuthority,
        _metadata: ExecutionMetadata,
    ) -> Result<EvidenceDocumentUploadGrantAuthority, EvidenceDocumentUploadGrantAuthorityError>
    {
        let verified = self
            .decryptor
            .decrypt::<EvidenceDocumentUploadGrantClaims>(query.credential.expose_secret())
            .map_err(|_| EvidenceDocumentUploadGrantAuthorityError::Unavailable)?;
        let claims = VerifiedEvidenceDocumentUploadGrantClaims::try_from(verified)
            .map_err(|_| EvidenceDocumentUploadGrantAuthorityError::Unavailable)?;
        let coverage = CoverageWindow::new(claims.valid_from, claims.valid_until)
            .map_err(|_| EvidenceDocumentUploadGrantAuthorityError::Unavailable)?;
        Ok(EvidenceDocumentUploadGrantAuthority::new(
            claims.grant_id.into(),
            claims.workspace_id.into(),
            claims.evidence_id.into(),
            coverage,
            claims.issued_by_user_id.into(),
            claims.issued_via_agent_connection_id.into(),
            claims.expires_at,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceDocumentUploadGrantAuthorityError {
    #[error("document upload grant is unavailable")]
    Unavailable,
}

impl From<EvidenceDocumentUploadGrantError> for EvidenceDocumentUploadGrantAuthorityError {
    fn from(_: EvidenceDocumentUploadGrantError) -> Self {
        Self::Unavailable
    }
}
