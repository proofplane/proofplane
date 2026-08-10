use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    json::{assert_rfc3339, object_keys},
    scenario::types::TestPolicy,
};

pub struct ExpectedPolicyDocument<'a> {
    pub user_id: Uuid,
    pub filename: &'a str,
    pub bytes: &'a [u8],
    pub upload_status: &'a str,
}

#[track_caller]
pub fn assert_policy_read_model(
    read_model: &Value,
    policy: &TestPolicy,
    expected_document: Option<ExpectedPolicyDocument<'_>>,
) -> Option<Uuid> {
    assert_eq!(
        object_keys(read_model),
        [
            "controls",
            "created_at",
            "description",
            "document",
            "id",
            "name",
            "updated_at",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(read_model["id"], policy.id.to_string());
    assert_eq!(read_model["name"], policy.name);
    assert_eq!(read_model["description"], json!(policy.description));
    assert_eq!(read_model["controls"], json!([]));
    assert_rfc3339(&read_model["created_at"]);
    assert_rfc3339(&read_model["updated_at"]);

    let Some(expected) = expected_document else {
        assert_eq!(read_model["document"], Value::Null);
        return None;
    };
    let document = &read_model["document"];
    assert_eq!(
        object_keys(document),
        [
            "checksum_crc32c",
            "checksum_sha256",
            "content_length",
            "content_type",
            "created_at",
            "created_by_user_id",
            "filename",
            "id",
            "upload_status",
        ]
        .into_iter()
        .collect()
    );
    let document_id = Uuid::parse_str(
        document["id"]
            .as_str()
            .expect("policy document id is a string"),
    )
    .expect("policy document id is a UUID");
    assert_eq!(document["created_by_user_id"], expected.user_id.to_string());
    assert_eq!(document["filename"], expected.filename);
    assert_eq!(document["content_type"], "text/plain");
    assert_eq!(document["content_length"], expected.bytes.len());
    assert_eq!(
        document["checksum_sha256"],
        hex::encode(Sha256::digest(expected.bytes))
    );
    assert_eq!(
        document["checksum_crc32c"],
        BASE64_STANDARD.encode(crc32c::crc32c(expected.bytes).to_be_bytes())
    );
    assert_eq!(document["upload_status"], expected.upload_status);
    assert_rfc3339(&document["created_at"]);
    Some(document_id)
}
