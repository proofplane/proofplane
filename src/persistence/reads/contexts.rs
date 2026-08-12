use crate::domain::WorkspaceId;

use super::{
    AgentConnectionReads, AuditorAccessGrantReads, AuditorAuthTransactionReads, AuditorPortalReads,
    AuditorSessionReads, ControlReads, DocumentReads, EvidenceReads, EvidenceSubmissionReads,
    FrameworkReads, OAuthAuthorizationFlowReads, PolicyReads, ReadExecutor, UserReads,
    WorkspacePeopleReads, WorkspaceReads,
};

pub(crate) struct Reads<E> {
    executor: E,
}

impl<E> Reads<E> {
    pub(crate) fn new(executor: E) -> Self {
        Self { executor }
    }
}

impl<E: ReadExecutor> Reads<E> {
    pub(crate) fn frameworks(&self) -> FrameworkReads<'_, E> {
        FrameworkReads::new(&self.executor)
    }

    pub(crate) fn workspaces(&self) -> WorkspaceReads<'_, E> {
        WorkspaceReads::new(&self.executor)
    }

    pub(crate) fn users(&self) -> UserReads<'_, E> {
        UserReads::new(&self.executor)
    }

    pub(crate) fn oauth_authorization_flows(&self) -> OAuthAuthorizationFlowReads<'_, E> {
        OAuthAuthorizationFlowReads::new(&self.executor)
    }

    pub(crate) fn auditor_sessions(&self) -> AuditorSessionReads<'_, E> {
        AuditorSessionReads::new(&self.executor)
    }

    pub(crate) fn auditor_auth_transactions(&self) -> AuditorAuthTransactionReads<'_, E> {
        AuditorAuthTransactionReads::new(&self.executor)
    }

    pub(crate) fn auditor_access_grants(&self) -> AuditorAccessGrantReads<'_, E> {
        AuditorAccessGrantReads::new(&self.executor, None)
    }

    pub(crate) fn agent_connections(&self) -> AgentConnectionReads<'_, E> {
        AgentConnectionReads::new(&self.executor)
    }

    pub(crate) fn documents(&self) -> DocumentReads<'_, E> {
        DocumentReads::new(&self.executor)
    }

    pub(crate) fn workspace_people(&self) -> WorkspacePeopleReads<'_, E> {
        WorkspacePeopleReads::new(&self.executor)
    }

    pub(crate) fn workspace(&self, workspace_id: WorkspaceId) -> WorkspaceScopedReads<&E> {
        WorkspaceScopedReads::new(&self.executor, workspace_id)
    }
}

pub(crate) struct WorkspaceScopedReads<E> {
    executor: E,
    workspace_id: WorkspaceId,
}

impl<E> WorkspaceScopedReads<E> {
    pub(crate) fn new(executor: E, workspace_id: WorkspaceId) -> Self {
        Self {
            executor,
            workspace_id,
        }
    }
}

impl<E: ReadExecutor> WorkspaceScopedReads<E> {
    pub(crate) fn frameworks(&self) -> FrameworkReads<'_, E> {
        FrameworkReads::new(&self.executor)
    }

    pub(crate) fn controls(&self) -> ControlReads<'_, E> {
        ControlReads::new(&self.executor, self.workspace_id)
    }

    pub(crate) fn evidence(&self) -> EvidenceReads<'_, E> {
        EvidenceReads::new(&self.executor, self.workspace_id)
    }

    pub(crate) fn policies(&self) -> PolicyReads<'_, E> {
        PolicyReads::new(&self.executor, self.workspace_id)
    }

    pub(crate) fn evidence_submissions(&self) -> EvidenceSubmissionReads<'_, E> {
        EvidenceSubmissionReads::new(&self.executor, self.workspace_id)
    }

    pub(crate) fn auditor_access_grants(&self) -> AuditorAccessGrantReads<'_, E> {
        AuditorAccessGrantReads::new(&self.executor, Some(self.workspace_id))
    }

    pub(crate) fn auditor_portal(&self) -> AuditorPortalReads<'_, E> {
        AuditorPortalReads::new(&self.executor, self.workspace_id)
    }
}
