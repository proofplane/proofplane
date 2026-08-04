use std::collections::BTreeSet;

use super::*;

#[track_caller]
pub(super) fn policy_machine_transfer(prepared: &Value) -> MachineTransfer {
    assert_eq!(
        object_keys(prepared),
        ["upload", "upload_id"].into_iter().collect()
    );
    parse_machine_transfer(
        &prepared["upload_id"],
        &prepared["upload"],
        CONTENT_TYPE,
        "agent-policy-document-uploads",
    )
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
pub(super) fn assert_policy_conflict(result: &HttpResult) {
    assert_http_error(
        result,
        StatusCode::CONFLICT,
        "policy_document_exists",
        "this policy already has a current document",
        json!([]),
    );
}

#[track_caller]
pub(super) fn assert_pending_result(
    result: &HttpResult,
    expected_status: StatusCode,
    policy_id: Uuid,
) -> Uuid {
    assert_eq!(result.status, expected_status);
    let document_id = uuid_at(&result.body["document_id"], "pending document id");
    assert_eq!(
        result.body,
        json!({
            "policy_id": policy_id,
            "document_id": document_id,
            "upload_status": "pending",
        })
    );
    document_id
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
    let expected_keys: BTreeSet<_> = [
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
    .collect();
    assert_eq!(object_keys(fields), expected_keys);
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

#[track_caller]
pub(super) fn assert_policy_document_exists(error: &McpError) {
    assert_eq!(error.code, ErrorCode(-32000));
    assert_eq!(
        error.data,
        json!({
            "problem": {
                "code": "policy_document_exists",
                "message": "policy already has a current document; call get_policy to inspect it",
            }
        })
    );
}

#[track_caller]
pub(super) fn assert_browser_conflict(response: &axum_test::TestResponse) {
    response.assert_status(StatusCode::CONFLICT);
    assert_eq!(
        notice_section(&response.text()),
        r#"<section class="notice" role="alert"><strong>Upload failed: this policy already has a current document</strong><p>Review the message above, then try again.</p></section>"#
    );
}

fn notice_section(html: &str) -> String {
    let opening = r#"<section class="notice" role="alert">"#;
    let start = html.find(opening).expect("HTML notice opens");
    let end = html[start..]
        .find("</section>")
        .map(|offset| start + offset + "</section>".len())
        .expect("HTML notice closes");
    html[start..end].to_owned()
}
