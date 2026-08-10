use std::{sync::Arc, time::Duration};

use chrono::Utc;
use secrecy::SecretString;
use uuid::Uuid;

use crate::{
    application::ExecutionMetadata,
    authentication::{
        paseto::{
            AgentEvidenceUploadGrantClaims, AgentEvidenceUploadGrantEncryptor, RegisteredClaims,
        },
        AgentConnectionContext,
    },
    domain::{
        AgentEvidenceUploadDeclaration, AgentEvidenceUploadGrant, AgentEvidenceUploadGrantId,
        CoverageWindow, EvidenceId, EvidenceSubmissionId, WorkspacePermission,
    },
    persistence::Postgres,
};

const GRANT_TTL: Duration = Duration::from_secs(5 * 60);
pub use crate::authentication::paseto::AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE;

#[derive(Debug, Clone)]
pub struct IssueAgentEvidenceUploadGrant {
    pub connection: AgentConnectionContext,
    pub evidence_id: EvidenceId,
    pub coverage: CoverageWindow,
    pub declaration: AgentEvidenceUploadDeclaration,
}

#[derive(Clone)]
pub struct IssueAgentEvidenceUploadGrantHandler {
    repository: Arc<Postgres>,
    encryptor: AgentEvidenceUploadGrantEncryptor,
}

#[derive(Debug)]
pub struct IssuedAgentEvidenceUploadGrant {
    pub grant: AgentEvidenceUploadGrant,
    pub credential: SecretString,
}

impl IssueAgentEvidenceUploadGrantHandler {
    pub fn new(repository: Arc<Postgres>, encryptor: AgentEvidenceUploadGrantEncryptor) -> Self {
        Self {
            repository,
            encryptor,
        }
    }

    pub async fn handle(
        &self,
        command: IssueAgentEvidenceUploadGrant,
        _metadata: ExecutionMetadata,
    ) -> Result<IssuedAgentEvidenceUploadGrant, AgentEvidenceUploadGrantError> {
        if !command
            .connection
            .permissions
            .has(WorkspacePermission::WriteEvidenceSubmissions)
        {
            return Err(AgentEvidenceUploadGrantError::Unavailable);
        }

        let encryptor = self.encryptor.clone();
        let outcome = self
            .repository
            .in_unit_of_work(async move |unit_of_work| {
                let workspace = unit_of_work.workspace(command.connection.workspace_id);
                if workspace
                    .reads()
                    .evidence()
                    .get(command.evidence_id)
                    .await?
                    .is_none()
                {
                    return Ok(IssueOutcome::Unavailable);
                }

                let upload_id = AgentEvidenceUploadGrantId::from(Uuid::new_v4());
                let submission_id = EvidenceSubmissionId::from(Uuid::new_v4());
                let issued_at = Utc::now();
                let Ok(ttl) = chrono::Duration::from_std(GRANT_TTL) else {
                    return Ok(IssueOutcome::Internal);
                };
                let expires_at = issued_at + ttl;
                let issued = match encryptor.encrypt(
                    RegisteredClaims {
                        subject: Uuid::from(command.connection.user_id),
                        token_id: Uuid::from(upload_id),
                        expires_at,
                    },
                    &AgentEvidenceUploadGrantClaims::new(
                        upload_id.into(),
                        command.connection.workspace_id.into(),
                        command.evidence_id.into(),
                        submission_id.into(),
                        command.connection.user_id.into(),
                        command.connection.connection_id.into(),
                    ),
                ) {
                    Ok(issued) => issued,
                    Err(_) => return Ok(IssueOutcome::Internal),
                };
                let grant = match AgentEvidenceUploadGrant::issue(
                    upload_id,
                    submission_id,
                    command.connection.workspace_id,
                    command.evidence_id,
                    command.coverage,
                    command.declaration,
                    command.connection.user_id,
                    command.connection.connection_id,
                    issued_at,
                    issued.expires_at,
                ) {
                    Ok(grant) => grant,
                    Err(_) => return Ok(IssueOutcome::Internal),
                };
                let repository = workspace.aggregates().agent_evidence_upload_grants();
                repository.save(&grant).await?;
                let grant = repository
                    .get(grant.id(), grant.workspace_id())
                    .await?
                    .ok_or(crate::persistence::Error::InvariantViolation(
                        "saved machine upload grant must be readable",
                    ))?;

                Ok(IssueOutcome::Issued(Box::new(
                    IssuedAgentEvidenceUploadGrant {
                        grant,
                        credential: SecretString::from(issued.token),
                    },
                )))
            })
            .await?;

        match outcome {
            IssueOutcome::Issued(issued) => Ok(*issued),
            IssueOutcome::Unavailable => Err(AgentEvidenceUploadGrantError::Unavailable),
            IssueOutcome::Internal => Err(AgentEvidenceUploadGrantError::Internal),
        }
    }
}

enum IssueOutcome {
    Issued(Box<IssuedAgentEvidenceUploadGrant>),
    Unavailable,
    Internal,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentEvidenceUploadGrantError {
    #[error("agent evidence upload grant is unavailable")]
    Unavailable,
    #[error("internal agent evidence upload grant error")]
    Internal,
    #[error("repository error")]
    Repository(#[from] crate::persistence::Error),
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use secrecy::SecretString;
    use url::Url;

    use crate::{
        config::{PasetoUploadGrantConfig, PasetoUploadGrantKey},
        domain::{Sha256Digest, WorkspacePermissions},
        persistence::test_support::{self, TestWorkspace},
    };

    use super::*;

    fn handler(postgres: &Arc<Postgres>) -> IssueAgentEvidenceUploadGrantHandler {
        let config = PasetoUploadGrantConfig {
            active_key_id: "unit-upload-grant-001".to_owned(),
            keys: vec![PasetoUploadGrantKey {
                id: "unit-upload-grant-001".to_owned(),
                secret: SecretString::from("k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY"),
            }],
        };

        IssueAgentEvidenceUploadGrantHandler::new(
            postgres.clone(),
            AgentEvidenceUploadGrantEncryptor::from_config(
                Url::parse("https://api.proofplane.test/").expect("issuer parses"),
                AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE,
                &config,
            )
            .expect("upload grant encryptor initializes"),
        )
    }

    fn command(
        workspace: &TestWorkspace,
        evidence_id: EvidenceId,
        permissions: WorkspacePermissions,
    ) -> IssueAgentEvidenceUploadGrant {
        let valid_from = Utc::now();

        IssueAgentEvidenceUploadGrant {
            connection: AgentConnectionContext {
                user_id: workspace.user_id,
                connection_id: workspace.agent_connection_id,
                workspace_id: workspace.workspace_id,
                permissions,
            },
            evidence_id,
            coverage: CoverageWindow::new(valid_from, valid_from + Duration::days(1))
                .expect("coverage window is valid"),
            declaration: AgentEvidenceUploadDeclaration::new(
                "evidence.pdf".to_owned(),
                "application/pdf".to_owned(),
                3,
                Some(hex::encode(Sha256Digest::digest(b"abc").as_bytes())),
                100,
            )
            .into_result()
            .expect("declaration is valid"),
        }
    }

    #[tokio::test]
    async fn persistence_failure_returns_no_credential_and_leaves_no_grant() {
        let postgres = Arc::new(test_support::database().await);
        let workspace = test_support::workspace(&postgres, "Owner").await;
        let evidence_id =
            test_support::evidence(&postgres, workspace.workspace_id, "Access review").await;

        postgres
            .get()
            .await
            .expect("database connection opens")
            .batch_execute(
                r#"
CREATE FUNCTION fail_machine_grant_save() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'injected machine grant save failure';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER fail_machine_grant_save
BEFORE INSERT ON agent_evidence_upload_grants
FOR EACH ROW EXECUTE FUNCTION fail_machine_grant_save();
"#,
            )
            .await
            .expect("failure trigger installs");

        let result = handler(&postgres)
            .handle(
                command(&workspace, evidence_id, WorkspacePermissions::all()),
                ExecutionMetadata::for_request(Uuid::new_v4()),
            )
            .await;

        assert!(matches!(
            result,
            Err(AgentEvidenceUploadGrantError::Repository(_))
        ));

        let remaining: i64 = postgres
            .get()
            .await
            .expect("database connection opens")
            .query_one("SELECT count(*) FROM agent_evidence_upload_grants", &[])
            .await
            .expect("grant count loads")
            .get(0);
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn a_grant_is_issued_for_evidence_in_every_status() {
        let postgres = Arc::new(test_support::database().await);
        let workspace = test_support::workspace(&postgres, "Owner").await;
        let handler = handler(&postgres);

        for status in ["active", "paused", "retired"] {
            let evidence_id = test_support::evidence_with_status(
                &postgres,
                workspace.workspace_id,
                status,
                status,
            )
            .await;

            let issued = handler
                .handle(
                    command(&workspace, evidence_id, WorkspacePermissions::all()),
                    ExecutionMetadata::for_request(Uuid::new_v4()),
                )
                .await
                .unwrap_or_else(|_| panic!("machine grant issues for {status} evidence"));

            assert_eq!(issued.grant.evidence_id(), evidence_id);
        }
    }

    #[tokio::test]
    async fn a_connection_without_submission_write_permission_is_told_nothing() {
        let postgres = Arc::new(test_support::database().await);
        let workspace = test_support::workspace(&postgres, "Owner").await;
        let evidence_id =
            test_support::evidence(&postgres, workspace.workspace_id, "Access review").await;

        let result = handler(&postgres)
            .handle(
                command(&workspace, evidence_id, WorkspacePermissions::none()),
                ExecutionMetadata::for_request(Uuid::new_v4()),
            )
            .await;

        assert!(matches!(
            result,
            Err(AgentEvidenceUploadGrantError::Unavailable)
        ));
    }
}
