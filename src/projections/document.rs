use crate::domain::{Document, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentDownloadCandidate {
    pub workspace_id: WorkspaceId,
    pub document: Document,
}
