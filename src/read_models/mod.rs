//! Purpose-built, read-only data shapes returned by application queries and
//! command responses.

mod agent_connection;
mod auditor_access_grant;
mod auditor_portal;
mod control;
mod document;
mod evidence;
mod evidence_submission;
mod policy;
mod workspace;
mod workspace_people;

pub use agent_connection::{
    AgentConnectionAuthority, ReusableAgentConnection, UserAgentConnectionSummary,
};
pub use auditor_access_grant::AuditorAccessGrantSummary;
pub use auditor_portal::{
    AuditorPortalControl, AuditorPortalDocument, AuditorPortalEvidence, AuditorPortalPolicy,
    AuditorPortalPolicyDocument, AuditorPortalPolicyDocumentStatus, AuditorPortalPolicySummary,
    AuditorPortalReadModel, AuditorPortalSubmission,
};
pub use control::{
    ControlDetail, ControlSummary, EvidenceControlMapping, FrameworkDetail,
    FrameworkRequirementDetail,
};
pub use document::{DocumentDownloadCandidate, TypedDocumentUploadWork};
pub use evidence::{ControlEvidenceMapping, EvidenceDetail};
pub use evidence_submission::EvidenceSubmissionDetail;
pub use policy::{
    ControlPolicyMapping, PolicyCatalogEntry, PolicyControlMapping, PolicyDetail,
    PolicyDocumentDetail, PolicyDocumentStatus, PolicyDocumentUploadEligibility, PolicySummary,
};
pub use workspace::{WorkspaceDetails, WorkspaceWithRole};
pub use workspace_people::{
    PendingWorkspaceInvitation, WorkspaceInvitationPreviewSource, WorkspacePeople, WorkspacePerson,
};
