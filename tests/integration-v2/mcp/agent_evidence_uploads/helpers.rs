use super::*;
use std::ops::Deref;

pub(super) struct EvidenceMachineTransfer {
    transfer: MachineTransfer,
    pub(super) submission_id: Uuid,
}

impl Deref for EvidenceMachineTransfer {
    type Target = MachineTransfer;

    fn deref(&self) -> &Self::Target {
        &self.transfer
    }
}

#[track_caller]
pub(super) fn machine_transfer(
    prepared: &Value,
    expected_content_type: &str,
) -> EvidenceMachineTransfer {
    assert_eq!(
        object_keys(prepared),
        ["submission_id", "upload", "upload_id"]
            .into_iter()
            .collect()
    );
    let submission_id = uuid_at(&prepared["submission_id"], "submission id");
    EvidenceMachineTransfer {
        transfer: parse_machine_transfer(
            &prepared["upload_id"],
            &prepared["upload"],
            expected_content_type,
            "agent-evidence-uploads",
        ),
        submission_id,
    }
}

pub(super) async fn assert_preallocated_submission_is_concealed(
    client: &McpClient,
    evidence_id: Uuid,
    submission_id: Uuid,
) {
    let concealed = client
        .call_tool_error(
            "get_evidence_submission",
            json!({ "submission_id": submission_id }),
        )
        .await;
    assert_not_found(&concealed);
    let listed = client
        .call_tool(
            "list_evidence_submissions",
            json!({ "evidence_id": evidence_id }),
        )
        .await;
    assert_eq!(listed, json!({ "submissions": [] }));
}

#[track_caller]
pub(super) fn assert_http_error(
    result: &HttpResult,
    status: StatusCode,
    code: &str,
    message: &str,
    details: Value,
) {
    assert_eq!(result.status, status);
    assert_eq!(
        result.body,
        json!({
            "error": {
                "code": code,
                "message": message,
                "details": details,
            }
        })
    );
}

#[track_caller]
pub(super) fn assert_pending_result(
    result: &HttpResult,
    expected_status: StatusCode,
    descriptor: &EvidenceMachineTransfer,
) -> Uuid {
    assert_eq!(result.status, expected_status);
    let document_id = uuid_at(&result.body["document_id"], "pending document id");
    assert_eq!(
        result.body,
        json!({
            "submission_id": descriptor.submission_id,
            "document_id": document_id,
            "upload_status": "pending",
        })
    );
    document_id
}

#[allow(clippy::too_many_arguments)]
#[track_caller]
pub(super) fn assert_submission_projection(
    detail: &Value,
    submission_id: Uuid,
    document_id: Uuid,
    evidence_id: Uuid,
    user_id: Uuid,
    connection_id: Uuid,
    filename: &str,
    bytes: &[u8],
    upload_status: &str,
) {
    assert_eq!(
        object_keys(detail),
        ["document", "submission"].into_iter().collect()
    );
    let submission = &detail["submission"];
    assert_eq!(
        object_keys(submission),
        [
            "evidence_id",
            "id",
            "received_at",
            "submitted_by",
            "valid_from",
            "valid_until",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(submission["id"], submission_id.to_string());
    assert_eq!(submission["evidence_id"], evidence_id.to_string());
    assert_eq!(submission["valid_from"], VALID_FROM);
    assert_eq!(submission["valid_until"], VALID_UNTIL);
    assert_rfc3339(&submission["received_at"]);
    assert_eq!(
        object_keys(&submission["submitted_by"]),
        ["agent_connection_id", "user_id"].into_iter().collect()
    );
    assert_eq!(submission["submitted_by"]["user_id"], user_id.to_string());
    assert_eq!(
        submission["submitted_by"]["agent_connection_id"],
        connection_id.to_string()
    );

    let document = &detail["document"];
    assert_eq!(
        object_keys(document),
        [
            "checksum_crc32c",
            "checksum_sha256",
            "content_length",
            "content_type",
            "created_by_user_id",
            "evidence_submission_id",
            "filename",
            "id",
            "upload_status",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(document["id"], document_id.to_string());
    assert_eq!(
        document["evidence_submission_id"],
        submission_id.to_string()
    );
    assert_eq!(document["created_by_user_id"], user_id.to_string());
    assert_eq!(document["filename"], filename);
    assert_eq!(document["content_type"], CONTENT_TYPE);
    assert_eq!(document["content_length"], bytes.len());
    assert_eq!(document["checksum_sha256"], sha256(bytes));
    assert_eq!(
        document["checksum_crc32c"],
        BASE64_STANDARD.encode(crc32c::crc32c(bytes).to_be_bytes())
    );
    assert_eq!(document["upload_status"], upload_status);
}

#[allow(clippy::too_many_arguments)]
#[track_caller]
pub(super) fn assert_upload_audit_event(
    record: &Value,
    request_id: Uuid,
    event_name: &str,
    client_type: &str,
    operation: &str,
    user_id: Uuid,
    connection_id: Uuid,
    workspace_id: Uuid,
    object_type: &str,
    object_id: Uuid,
    metadata: Value,
) {
    assert_eq!(
        object_keys(record),
        ["fields", "level", "target", "timestamp"]
            .into_iter()
            .collect()
    );
    assert_eq!(record["level"], "INFO");
    assert_eq!(record["target"], "proofplane::audit");
    assert_rfc3339(&record["timestamp"]);
    let fields = &record["fields"];
    assert_eq!(
        object_keys(fields),
        [
            "actor_type",
            "agent_connection_id",
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "object_id",
            "object_type",
            "operation",
            "outcome",
            "request_id",
            "type",
            "user_id",
            "workspace_id",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(fields["type"], "audit_log");
    uuid_at(&fields["event_id"], "audit event id");
    assert_eq!(fields["event_name"], event_name);
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["actor_type"], "agent_connection");
    assert_eq!(fields["user_id"], user_id.to_string());
    assert_eq!(fields["agent_connection_id"], connection_id.to_string());
    assert_eq!(fields["client_type"], client_type);
    assert_eq!(fields["operation"], operation);
    assert_eq!(fields["workspace_id"], workspace_id.to_string());
    assert_eq!(fields["request_id"], request_id.to_string());
    assert_eq!(fields["object_type"], object_type);
    assert_eq!(fields["object_id"], object_id.to_string());
    assert_eq!(
        serde_json::from_str::<Value>(
            fields["metadata"]
                .as_str()
                .expect("audit metadata is serialized JSON")
        )
        .expect("audit metadata parses"),
        metadata
    );
}
