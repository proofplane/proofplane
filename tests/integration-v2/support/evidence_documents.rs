use serde_json::Value;
use uuid::Uuid;

// Written the way the server formats returned timestamps, so the same
// constants can be sent and asserted on.
pub const VALID_FROM: &str = "2026-01-01T00:00:00.000Z";
pub const VALID_UNTIL: &str = "2026-03-31T23:59:59.000Z";

pub fn file_path(submission: &Value) -> String {
    let submission_id = Uuid::parse_str(
        submission["submission"]["id"]
            .as_str()
            .expect("submission id is a string"),
    )
    .expect("submission id is a UUID");
    let document_id = Uuid::parse_str(
        submission["document"]["id"]
            .as_str()
            .expect("document id is a string"),
    )
    .expect("document id is a UUID");

    file_path_for_ids(submission_id, document_id)
}

pub fn file_path_for_ids(submission_id: Uuid, document_id: Uuid) -> String {
    format!("/evidence-document-uploads/files/{submission_id}/{document_id}")
}

pub fn download_path(submission: &Value) -> String {
    format!("{}/download", file_path(submission))
}

pub fn archive_path(submission: &Value) -> String {
    format!("{}/archive", file_path(submission))
}

pub fn archive_path_for_ids(submission_id: Uuid, document_id: Uuid) -> String {
    format!("{}/archive", file_path_for_ids(submission_id, document_id))
}
