use uuid::Uuid;

pub(super) const EVIDENCE_UPLOAD_PATH: &str = "/evidence-document-uploads/files";
pub(super) const POLICY_UPLOAD_PATH: &str = "/policy-document-uploads/files";
pub(super) const PORTAL_DATA_PATH: &str = "/auditor-access/portal/data";

pub(super) fn policy_archive_path(document_id: Uuid) -> String {
    format!("{POLICY_UPLOAD_PATH}/{document_id}/archive")
}
