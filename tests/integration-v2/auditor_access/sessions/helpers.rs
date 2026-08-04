use axum_test::TestResponse;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::support::json::{assert_rfc3339, object_keys};

#[track_caller]
pub(super) fn assert_unavailable_page(html: &str) {
    assert_eq!(
        main_content(html),
        r#"<main class="narrow">
<p class="eyebrow">Access unavailable</p>
<h1>This auditor portal is not available</h1>
<p class="lede">The link or session may be expired or revoked. Ask the Proofplane workspace owner for a new auditor access link.</p>
</main>"#
    );
}

#[track_caller]
pub(super) fn assert_authentication_rejected_page(html: &str) {
    assert_eq!(
        main_content(html),
        r#"<main class="narrow">
<p class="eyebrow">Verification failed</p>
<h1>We couldn&#39;t verify this access request</h1>
<p class="lede">Return to your invitation and try again.</p>
</main>"#
    );
}

#[track_caller]
pub(super) fn assert_authentication_unavailable_page(html: &str) {
    assert_eq!(
        main_content(html),
        r#"<main class="narrow">
<p class="eyebrow">Verification unavailable</p>
<h1>Email verification is temporarily unavailable</h1>
<p class="lede">Please try again from your invitation.</p>
</main>"#
    );
}

#[track_caller]
pub(super) fn assert_portal_data_not_found(response: TestResponse) {
    response.assert_status_not_found();
    assert_not_found_json(&response);
}

#[track_caller]
pub(super) fn assert_not_found_json(response: &TestResponse) {
    assert_eq!(
        response.json::<Value>(),
        json!({
            "error": {
                "code": "not_found",
                "message": "route not found",
                "details": [],
            }
        })
    );
}

#[track_caller]
pub(super) fn assert_auth_started_audit(
    record: &Value,
    workspace_id: Uuid,
    grant_id: &str,
    request_id: Uuid,
) -> Uuid {
    let fields = assert_common_audit(
        record,
        &[
            "actor_type",
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "object_id",
            "object_type",
            "operation",
            "outcome",
            "request_id",
            "system_name",
            "type",
            "workspace_id",
        ],
    );
    assert_eq!(fields["event_name"], "auditor_access_auth.started");
    assert_eq!(fields["operation"], "start_auditor_authentication");
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["workspace_id"], workspace_id.to_string());
    assert_eq!(fields["request_id"], request_id.to_string());
    assert_eq!(fields["object_type"], "auditor_access_grant");
    assert_eq!(fields["object_id"], grant_id);
    let metadata: Value =
        serde_json::from_str(fields["metadata"].as_str().expect("audit metadata is text"))
            .expect("audit metadata is JSON");
    assert_eq!(
        object_keys(&metadata),
        ["transaction_id"].into_iter().collect()
    );
    Uuid::parse_str(
        metadata["transaction_id"]
            .as_str()
            .expect("transaction id is text"),
    )
    .expect("transaction id is a UUID")
}

#[track_caller]
pub(super) fn assert_auth_completed_audit(
    record: &Value,
    workspace_id: Uuid,
    transaction_id: Uuid,
    auth0_subject: &str,
    request_id: Uuid,
) {
    let fields = assert_common_audit(
        record,
        &[
            "actor_type",
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "object_id",
            "object_type",
            "operation",
            "outcome",
            "request_id",
            "system_name",
            "type",
            "workspace_id",
        ],
    );
    assert_eq!(fields["event_name"], "auditor_access_auth.completed");
    assert_eq!(fields["operation"], "complete_auditor_authentication");
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["workspace_id"], workspace_id.to_string());
    assert_eq!(fields["request_id"], request_id.to_string());
    assert_eq!(fields["object_type"], "auditor_auth_transaction");
    assert_eq!(fields["object_id"], transaction_id.to_string());
    assert_eq!(
        fields["metadata"],
        format!(r#"{{"auth0_subject":"{auth0_subject}"}}"#)
    );
}

#[track_caller]
pub(super) fn assert_session_created_audit(
    record: &Value,
    workspace_id: Uuid,
    auth0_subject: &str,
    request_id: Uuid,
) {
    let fields = assert_common_audit(
        record,
        &[
            "actor_type",
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "object_id",
            "object_type",
            "operation",
            "outcome",
            "request_id",
            "system_name",
            "type",
            "workspace_id",
        ],
    );
    assert_eq!(fields["event_name"], "auditor_access_session.created");
    assert_eq!(fields["operation"], "create_auditor_access_session");
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["workspace_id"], workspace_id.to_string());
    assert_eq!(fields["request_id"], request_id.to_string());
    assert_eq!(fields["object_type"], "auditor_access_session");
    Uuid::parse_str(fields["object_id"].as_str().expect("session id is text"))
        .expect("session id is a UUID");
    assert_eq!(
        fields["metadata"],
        format!(r#"{{"auth0_subject":"{auth0_subject}"}}"#)
    );
}

#[track_caller]
pub(super) fn assert_auth_failure_audit(record: &Value, category: &str, request_id: Uuid) {
    let fields = assert_common_audit(
        record,
        &[
            "actor_type",
            "client_type",
            "event_id",
            "event_name",
            "metadata",
            "operation",
            "outcome",
            "request_id",
            "system_name",
            "type",
        ],
    );
    assert_eq!(fields["event_name"], "auditor_access_auth.completed");
    assert_eq!(fields["operation"], "complete_auditor_authentication");
    assert_eq!(fields["outcome"], "failure");
    assert_eq!(fields["request_id"], request_id.to_string());
    assert_eq!(
        fields["metadata"],
        format!(r#"{{"failure_category":"{category}"}}"#)
    );
}

fn assert_common_audit<'a>(record: &'a Value, expected_fields: &[&str]) -> &'a Value {
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
        expected_fields.iter().copied().collect()
    );
    assert_eq!(fields["type"], "audit_log");
    Uuid::parse_str(fields["event_id"].as_str().expect("event id is text"))
        .expect("event id is a UUID");
    assert_eq!(fields["actor_type"], "system");
    assert_eq!(fields["system_name"], "auditor_browser");
    assert_eq!(fields["client_type"], "rest");
    fields
}

fn main_content(html: &str) -> String {
    let start = html.find("<main").expect("page has main content");
    let main = &html[start..];
    let end = main
        .find("</main>")
        .map(|index| index + "</main>".len())
        .expect("main content closes");
    main[..end].to_owned()
}
