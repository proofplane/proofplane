use crate::{
    authentication::paseto::{
        AgentEvidenceUploadGrantClaims, AgentEvidenceUploadGrantDecryptor,
        VerifiedAgentEvidenceUploadGrantClaims,
    },
    domain::AgentEvidenceUploadAuthority,
};

pub use crate::application::commands::issue_agent_evidence_upload_grant::{
    AgentEvidenceUploadGrantError, AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE,
};

#[derive(Clone)]
pub struct AgentEvidenceUploadCredentialVerifier {
    decryptor: AgentEvidenceUploadGrantDecryptor,
}

impl AgentEvidenceUploadCredentialVerifier {
    pub fn new(decryptor: AgentEvidenceUploadGrantDecryptor) -> Self {
        Self { decryptor }
    }

    pub fn verify(
        &self,
        credential: &str,
    ) -> Result<AgentEvidenceUploadAuthority, AgentEvidenceUploadGrantError> {
        let verified = self
            .decryptor
            .decrypt::<AgentEvidenceUploadGrantClaims>(credential)
            .map_err(|_| AgentEvidenceUploadGrantError::Unavailable)?;
        let claims = VerifiedAgentEvidenceUploadGrantClaims::try_from(verified)
            .map_err(|_| AgentEvidenceUploadGrantError::Unavailable)?;
        Ok(AgentEvidenceUploadAuthority::new(
            claims.upload_id.into(),
            claims.workspace_id.into(),
            claims.evidence_id.into(),
            claims.submission_id.into(),
            claims.issued_by_user_id.into(),
            claims.issued_via_agent_connection_id.into(),
            claims.expires_at,
        ))
    }
}
