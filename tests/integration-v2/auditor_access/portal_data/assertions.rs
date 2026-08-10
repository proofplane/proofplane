use serde_json::{json, Value};
use uuid::Uuid;

use crate::support::{
    json::{assert_rfc3339, object_keys},
    scenario::{
        types::{
            TestControl, TestEvidenceSubmission, TestFramework, TestFrameworkRequirement,
            TestPolicy, TestPolicyDocument,
        },
        Scenario,
    },
};

pub(super) fn assert_portal_envelope(body: &Value, workspace_name: &str, auditor_email: &str) {
    assert_eq!(
        object_keys(body),
        [
            "auditor_email",
            "controls",
            "framework_requirements",
            "policies",
            "workspace_name",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(body["workspace_name"], workspace_name);
    assert_eq!(body["auditor_email"], auditor_email);
    assert!(body["controls"].is_array());
    assert!(body["framework_requirements"].is_array());
    assert!(body["policies"].is_array());
}

#[track_caller]
pub(super) fn assert_framework_catalog(body: &Value, scenario: &Scenario) {
    let requirements = body["framework_requirements"]
        .as_array()
        .expect("framework requirements is an array");
    let soc2 = scenario.framework("soc2");
    assert_eq!(requirements.len(), 2);
    assert_requirement_read_model(&requirements[0], soc2.requirement("CC6.1"), soc2);
    assert_requirement_read_model(&requirements[1], soc2.requirement("CC7.1"), soc2);
}

#[track_caller]
fn assert_requirement_read_model(
    actual: &Value,
    expected: &TestFrameworkRequirement,
    framework: &TestFramework,
) {
    assert_eq!(
        object_keys(actual),
        [
            "code",
            "description",
            "framework_code",
            "framework_id",
            "framework_name",
            "id",
            "title",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(actual["id"], expected.id.to_string());
    assert_eq!(actual["framework_id"], expected.framework_id.to_string());
    assert_eq!(actual["framework_code"], framework.code);
    assert_eq!(actual["framework_name"], framework.name);
    assert_eq!(actual["code"], expected.code);
    assert_eq!(actual["title"], expected.title);
    assert_eq!(actual["description"], json!(expected.description));
}

#[track_caller]
pub(super) fn assert_control_read_model(actual: &Value, expected: &TestControl) {
    assert_eq!(
        object_keys(actual),
        [
            "code",
            "description",
            "evidence",
            "framework_requirements",
            "id",
            "policies",
            "title",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(actual["id"], expected.id.to_string());
    assert_eq!(actual["code"], expected.code);
    assert_eq!(actual["title"], expected.title);
    assert_eq!(actual["description"], json!(expected.description));
    assert!(actual["framework_requirements"].is_array());
    assert!(actual["evidence"].is_array());
    assert!(actual["policies"].is_array());
}

#[track_caller]
pub(super) fn assert_evidence_read_model(
    actual: &Value,
    evidence_id: Uuid,
    title: &str,
    rationale: &str,
) {
    assert_eq!(
        object_keys(actual),
        [
            "evidence",
            "mapping_created_at",
            "mapping_rationale",
            "submissions",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(actual["mapping_rationale"], rationale);
    assert_rfc3339(&actual["mapping_created_at"]);
    assert!(actual["submissions"].is_array());

    let evidence = &actual["evidence"];
    assert_eq!(
        object_keys(evidence),
        [
            "collection_instructions",
            "created_at",
            "description",
            "id",
            "status",
            "title",
            "updated_at",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(evidence["id"], evidence_id.to_string());
    assert_eq!(evidence["title"], title);
    assert_eq!(evidence["description"], format!("Collect {title}."));
    assert_eq!(
        evidence["collection_instructions"],
        format!("Upload {title}.")
    );
    assert_eq!(evidence["status"], "active");
    assert_rfc3339(&evidence["created_at"]);
    assert_rfc3339(&evidence["updated_at"]);
}

#[track_caller]
pub(super) fn assert_submission_read_model(
    actual: &Value,
    expected: &TestEvidenceSubmission,
    download_eligible: bool,
) {
    assert_eq!(
        object_keys(actual),
        ["document", "submission"].into_iter().collect()
    );
    let submission = &actual["submission"];
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
    assert_eq!(submission["id"], expected.id.to_string());
    assert_eq!(submission["evidence_id"], expected.evidence_id.to_string());
    assert_rfc3339(&submission["received_at"]);
    let actual_received_at = chrono::DateTime::parse_from_rfc3339(
        submission["received_at"]
            .as_str()
            .expect("portal received_at is text"),
    )
    .expect("portal received_at is RFC 3339");
    assert_eq!(
        actual_received_at.timestamp_millis(),
        expected.received_at.timestamp_millis()
    );
    assert_same_instant(&submission["valid_from"], &expected.valid_from);
    assert_same_instant(&submission["valid_until"], &expected.valid_until);
    assert_submitter_read_model(
        &submission["submitted_by"],
        expected.submitted_by_user_id,
        expected.submitted_by_agent_connection_id,
    );

    let document = &actual["document"];
    assert_eq!(
        object_keys(document),
        ["document_id", "download_eligible", "filename"]
            .into_iter()
            .collect()
    );
    assert_eq!(document["document_id"], expected.document_id.to_string());
    assert_eq!(document["filename"], expected.filename);
    assert_eq!(document["download_eligible"], download_eligible);
}

#[track_caller]
fn assert_submitter_read_model(actual: &Value, user_id: Uuid, agent_connection_id: Uuid) {
    assert_eq!(
        object_keys(actual),
        ["agent_connection_id", "user_id"].into_iter().collect()
    );
    assert_eq!(
        actual["agent_connection_id"],
        agent_connection_id.to_string()
    );
    assert_eq!(actual["user_id"], user_id.to_string());
}

#[track_caller]
fn assert_same_instant(actual: &Value, expected: &chrono::DateTime<chrono::FixedOffset>) {
    let actual =
        chrono::DateTime::parse_from_rfc3339(actual.as_str().expect("actual timestamp is text"))
            .expect("actual timestamp is RFC 3339");
    assert_eq!(actual.timestamp_millis(), expected.timestamp_millis());
}

#[track_caller]
pub(super) fn assert_policy_summary_read_model(
    actual: &Value,
    expected: &TestPolicy,
    document_eligibility: Option<bool>,
) {
    assert_eq!(
        object_keys(actual),
        ["description", "document", "id", "name"]
            .into_iter()
            .collect()
    );
    assert_eq!(actual["id"], expected.id.to_string());
    assert_eq!(actual["name"], expected.name);
    assert_eq!(actual["description"], json!(expected.description));
    match document_eligibility {
        Some(download_eligible) => {
            assert_eq!(
                object_keys(&actual["document"]),
                ["download_eligible"].into_iter().collect()
            );
            assert_eq!(actual["document"]["download_eligible"], download_eligible);
        }
        None => assert_eq!(actual["document"], Value::Null),
    }
}

#[track_caller]
pub(super) fn assert_policy_read_model(
    actual: &Value,
    expected: &TestPolicy,
    controls: &[&TestControl],
    document: Option<(&TestPolicyDocument, bool)>,
) {
    assert_eq!(
        object_keys(actual),
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
    assert_eq!(actual["id"], expected.id.to_string());
    assert_eq!(actual["name"], expected.name);
    assert_eq!(actual["description"], json!(expected.description));
    assert_rfc3339(&actual["created_at"]);
    assert_rfc3339(&actual["updated_at"]);
    let actual_controls = actual["controls"]
        .as_array()
        .expect("policy controls is an array");
    assert_eq!(actual_controls.len(), controls.len());
    for (actual_control, expected_control) in actual_controls.iter().zip(controls) {
        assert_eq!(
            object_keys(actual_control),
            ["code", "description", "id", "title"].into_iter().collect()
        );
        assert_eq!(actual_control["id"], expected_control.id.to_string());
        assert_eq!(actual_control["code"], expected_control.code);
        assert_eq!(actual_control["title"], expected_control.title);
        assert_eq!(actual_control["description"], expected_control.description);
    }
    match document {
        Some((expected_document, download_eligible)) => {
            let actual_document = &actual["document"];
            assert_eq!(
                object_keys(actual_document),
                [
                    "created_at",
                    "download_eligible",
                    "filename",
                    "id",
                    "policy_id",
                ]
                .into_iter()
                .collect()
            );
            assert_eq!(
                actual_document["id"],
                expected_document.document_id.to_string()
            );
            assert_eq!(
                actual_document["policy_id"],
                expected_document.policy_id.to_string()
            );
            assert_eq!(actual_document["filename"], expected_document.filename);
            assert_rfc3339(&actual_document["created_at"]);
            assert_same_instant(
                &actual_document["created_at"],
                &expected_document.created_at,
            );
            assert_eq!(actual_document["download_eligible"], download_eligible);
        }
        None => assert_eq!(actual["document"], Value::Null),
    }
}
