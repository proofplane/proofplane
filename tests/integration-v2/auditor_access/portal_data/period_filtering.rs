use proofplane::{domain::WorkspacePermission, routes::request_context::REQUEST_ID_HEADER};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use crate::support::{
    auditor_access::{authenticate_auditor, invite_token},
    harness,
    http::local_path,
    mcp::McpClient,
    oauth::authorize_agent_connection,
    scenario::ScenarioBuilder,
};

use super::{assertions::*, helpers::*};

#[tokio::test]
async fn period_overlap_filters_and_orders_sequential_submissions_newest_first() {
    let app = harness::app().await;
    let subject = "auth0|auditor-portal-period";
    let workspace_name = "Auditor Portal Period Filtering";
    let auditor_email = "auditor-portal-period@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Period-filtered evidence")
        .with_evidence_document(
            workspace_name,
            "Period-filtered evidence",
            "before-period.txt",
            b"before period bytes",
            "2026-01-01T00:00:00.000Z",
            "2026-01-31T23:59:59.000Z",
        )
        .with_evidence_document(
            workspace_name,
            "Period-filtered evidence",
            "start-overlapping.txt",
            b"start overlapping bytes",
            "2026-01-15T00:00:00.000Z",
            "2026-02-05T23:59:59.000Z",
        )
        .with_evidence_document(
            workspace_name,
            "Period-filtered evidence",
            "inside-period.txt",
            b"inside period bytes",
            "2026-02-05T00:00:00.000Z",
            "2026-02-10T23:59:59.000Z",
        )
        .with_evidence_document(
            workspace_name,
            "Period-filtered evidence",
            "end-overlapping.txt",
            b"end overlapping bytes",
            "2026-02-20T00:00:00.000Z",
            "2026-03-05T23:59:59.000Z",
        )
        .with_evidence_document(
            workspace_name,
            "Period-filtered evidence",
            "after-period.txt",
            b"after period bytes",
            "2026-03-01T00:00:00.000Z",
            "2026-03-31T23:59:59.000Z",
        )
        .with_control(workspace_name, "PP-PERIOD-01", "Period filtering control")
        .with_evidence_control_mapping(
            workspace_name,
            "Period-filtered evidence",
            "PP-PERIOD-01",
            "Includes submissions whose coverage overlaps the audit period.",
        )
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let evidence = workspace.evidence("Period-filtered evidence");
    let control = workspace.control("PP-PERIOD-01");

    let auditor_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Portal Period Access",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let auditor = McpClient::connect(app.mcp_server(), &auditor_token).await;
    let access = auditor
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2026-02-01T00:00:00Z",
                "period_end": "2026-02-28T23:59:59Z",
            }),
        )
        .await;
    let invite_url = Url::parse(access["url"].as_str().expect("auditor access URL is text"))
        .expect("auditor access URL parses");
    let invite_token = invite_token(&invite_url);
    app.app_server()
        .get(&local_path(invite_url.as_str()))
        .await
        .assert_status_ok();
    let auditor_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &invite_token,
        "auth0|auditor-portal-period-identity",
        auditor_email,
    )
    .await;

    let response = app
        .app_server()
        .get(PORTAL_DATA_PATH)
        .add_header("cookie", auditor_cookie)
        .add_header(REQUEST_ID_HEADER, Uuid::new_v4().to_string())
        .await;
    response.assert_status_ok();
    let body = response.json::<Value>();
    assert_portal_envelope(&body, workspace_name, auditor_email);
    assert_framework_catalog(&body, &scenario);
    assert_eq!(body["policies"], json!([]));

    let controls = body["controls"].as_array().expect("controls is an array");
    assert_eq!(controls.len(), 1);
    assert_control_projection(&controls[0], control);
    assert_eq!(controls[0]["framework_requirements"], json!([]));
    assert_eq!(controls[0]["policies"], json!([]));
    let mappings = controls[0]["evidence"]
        .as_array()
        .expect("evidence mappings is an array");
    assert_eq!(mappings.len(), 1);
    assert_evidence_projection(
        &mappings[0],
        evidence.id,
        "Period-filtered evidence",
        "Includes submissions whose coverage overlaps the audit period.",
    );
    let submissions = mappings[0]["submissions"]
        .as_array()
        .expect("portal submissions is an array");
    assert_eq!(submissions.len(), 3);
    assert_eq!(
        submissions
            .iter()
            .map(|submission| submission["document"]["filename"]
                .as_str()
                .expect("filename is text"))
            .collect::<Vec<_>>(),
        [
            "end-overlapping.txt",
            "inside-period.txt",
            "start-overlapping.txt",
        ]
    );
    let received_at = submissions
        .iter()
        .map(|submission| {
            chrono::DateTime::parse_from_rfc3339(
                submission["submission"]["received_at"]
                    .as_str()
                    .expect("received_at is text"),
            )
            .expect("received_at is RFC 3339")
        })
        .collect::<Vec<_>>();
    assert!(received_at.windows(2).all(|pair| pair[0] > pair[1]));
    let expected_submissions = [
        evidence.submission("end-overlapping.txt"),
        evidence.submission("inside-period.txt"),
        evidence.submission("start-overlapping.txt"),
    ];
    for (actual, expected) in submissions.iter().zip(expected_submissions) {
        assert_submission_projection(actual, expected, true);
    }
}
