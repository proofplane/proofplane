use crate::domain::{Document, DocumentIdentity, DocumentUploadStatus, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedDocumentUploadWork {
    pub workspace_id: WorkspaceId,
    pub identity: DocumentIdentity,
    pub filename: String,
    pub content_type: String,
    pub content_length: i64,
    pub object_key: String,
    pub checksum_sha256: String,
    pub upload_status: DocumentUploadStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentDownloadCandidate {
    pub workspace_id: WorkspaceId,
    pub document: Document,
}
