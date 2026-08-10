use http::StatusCode;
use proofplane::{domain::WorkspacePermission, routes::request_context::REQUEST_ID_HEADER};
use serde_json::{json, Value};
use url::Url;
use uuid::Uuid;

use crate::support::{
    auditor_access::{authenticate_auditor, invite_token},
    evidence_documents::archive_path_for_ids as evidence_archive_path,
    harness,
    http::{local_path, request_cookie},
    json::{assert_rfc3339, object_keys},
    mcp::McpClient,
    oauth::authorize_agent_connection,
    scenario::ScenarioBuilder,
};

use super::{assertions::*, helpers::*};

#[tokio::test]
async fn owner_archives_filter_documents_and_mcp_archive_filters_the_policy() {
    let app = harness::app().await;
    let subject = "auth0|auditor-portal-archives";
    let workspace_name = "Auditor Portal Archive Filtering";
    let auditor_email = "auditor-portal-archives@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .with_evidence(workspace_name, "Archive-filtered evidence")
        .with_evidence_document(
            workspace_name,
            "Archive-filtered evidence",
            "active-evidence.txt",
            b"active evidence bytes",
            "2026-01-01T00:00:00.000Z",
            "2026-03-31T23:59:59.000Z",
        )
        .with_evidence_document(
            workspace_name,
            "Archive-filtered evidence",
            "archived-evidence.txt",
            b"archived evidence bytes",
            "2026-01-01T00:00:00.000Z",
            "2026-03-31T23:59:59.000Z",
        )
        .with_control(workspace_name, "PP-ARCH-01", "Archive filtering control")
        .with_policy(workspace_name, "Active Document Policy")
        .with_policy_document(
            workspace_name,
            "Active Document Policy",
            "active-policy.txt",
            b"active policy bytes",
        )
        .with_policy(workspace_name, "Archived Document Policy")
        .with_policy_document(
            workspace_name,
            "Archived Document Policy",
            "archived-policy.txt",
            b"archived policy bytes",
        )
        .with_policy(workspace_name, "MCP Archived Policy")
        .with_evidence_control_mapping(
            workspace_name,
            "Archive-filtered evidence",
            "PP-ARCH-01",
            "Retains only active evidence documents.",
        )
        .with_policy_control_mapping(workspace_name, "Active Document Policy", "PP-ARCH-01")
        .with_policy_control_mapping(workspace_name, "Archived Document Policy", "PP-ARCH-01")
        .with_policy_control_mapping(workspace_name, "MCP Archived Policy", "PP-ARCH-01")
        .build()
        .await;
    let workspace = scenario.workspace(workspace_name);
    let workspace_id = workspace.id;
    let evidence = workspace.evidence("Archive-filtered evidence");
    let control = workspace.control("PP-ARCH-01");
    let active_policy = workspace.policy("Active Document Policy");
    let document_archived_policy = workspace.policy("Archived Document Policy");
    let mcp_archived_policy = workspace.policy("MCP Archived Policy");
    let active_evidence_submission = evidence.submission("active-evidence.txt");
    let archived_evidence_submission = evidence.submission("archived-evidence.txt");
    let active_policy_document = active_policy.document();
    let archived_policy_document = document_archived_policy.document();

    let owner_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Portal Archive Owner",
        &WorkspacePermission::ALL,
    )
    .await;
    let owner = McpClient::connect(app.mcp_server(), &owner_token).await;

    let evidence_grant = owner
        .call_tool(
            "manage_evidence_submissions",
            json!({
                "evidence_id": evidence.id,
                "valid_from": "2026-01-01T00:00:00.000Z",
                "valid_until": "2026-03-31T23:59:59.000Z",
            }),
        )
        .await;
    let evidence_redeemed = app
        .app_server()
        .get(&local_path(
            evidence_grant["url"]
                .as_str()
                .expect("evidence grant URL is a string"),
        ))
        .await;
    evidence_redeemed.assert_status(StatusCode::SEE_OTHER);
    let evidence_cookie = request_cookie(
        evidence_redeemed
            .header("set-cookie")
            .to_str()
            .expect("evidence session cookie is text"),
    );
    let evidence_archive = app
        .app_server()
        .post(&evidence_archive_path(
            archived_evidence_submission.id,
            archived_evidence_submission.document_id,
        ))
        .add_header("cookie", evidence_cookie)
        .add_header(REQUEST_ID_HEADER, Uuid::new_v4().to_string())
        .await;
    evidence_archive.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(
        evidence_archive.header("location"),
        "/evidence-document-uploads"
    );

    let archived_policy_grant = owner
        .call_tool(
            "manage_policy_document",
            json!({ "policy_id": document_archived_policy.id }),
        )
        .await;
    let archived_policy_redeemed = app
        .app_server()
        .get(&local_path(
            archived_policy_grant["url"]
                .as_str()
                .expect("archived policy grant URL is text"),
        ))
        .await;
    archived_policy_redeemed.assert_status(StatusCode::SEE_OTHER);
    let archived_policy_cookie = request_cookie(
        archived_policy_redeemed
            .header("set-cookie")
            .to_str()
            .expect("archived policy cookie is text"),
    );
    let policy_archive = app
        .app_server()
        .post(&policy_archive_path(archived_policy_document.document_id))
        .add_header("cookie", archived_policy_cookie)
        .add_header(REQUEST_ID_HEADER, Uuid::new_v4().to_string())
        .await;
    policy_archive.assert_status(StatusCode::SEE_OTHER);
    assert_eq!(
        policy_archive.header("location"),
        "/policy-document-uploads"
    );

    let archived_policy = owner
        .call_tool(
            "archive_policy",
            json!({ "policy_id": mcp_archived_policy.id }),
        )
        .await;
    assert_eq!(
        object_keys(&archived_policy),
        ["archived_at", "policy_id"].into_iter().collect()
    );
    assert_eq!(
        archived_policy["policy_id"],
        mcp_archived_policy.id.to_string()
    );
    assert_rfc3339(&archived_policy["archived_at"]);

    let auditor_token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Portal Archive Access",
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
                "period_start": "2026-01-01T00:00:00Z",
                "period_end": "2026-03-31T23:59:59Z",
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
        "auth0|auditor-portal-archives-identity",
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

    let controls = body["controls"].as_array().expect("controls is an array");
    assert_eq!(controls.len(), 1);
    assert_control_read_model(&controls[0], control);
    assert_eq!(controls[0]["framework_requirements"], json!([]));
    let mappings = controls[0]["evidence"]
        .as_array()
        .expect("evidence mappings is an array");
    assert_eq!(mappings.len(), 1);
    assert_evidence_read_model(
        &mappings[0],
        evidence.id,
        "Archive-filtered evidence",
        "Retains only active evidence documents.",
    );
    let submissions = mappings[0]["submissions"]
        .as_array()
        .expect("submissions is an array");
    assert_eq!(submissions.len(), 1);
    assert_submission_read_model(&submissions[0], active_evidence_submission, true);
    let control_policies = controls[0]["policies"]
        .as_array()
        .expect("control policies is an array");
    assert_eq!(control_policies.len(), 2);
    assert_eq!(
        control_policies
            .iter()
            .map(|policy| policy["name"].as_str().expect("policy name is text"))
            .collect::<Vec<_>>(),
        ["Active Document Policy", "Archived Document Policy"]
    );
    assert_policy_summary_read_model(&control_policies[0], active_policy, Some(true));
    assert_policy_summary_read_model(&control_policies[1], document_archived_policy, None);

    let policies = body["policies"].as_array().expect("policies is an array");
    assert_eq!(policies.len(), 2);
    assert_eq!(
        policies
            .iter()
            .map(|policy| policy["name"].as_str().expect("policy name is text"))
            .collect::<Vec<_>>(),
        ["Active Document Policy", "Archived Document Policy"]
    );
    assert_policy_read_model(
        &policies[0],
        active_policy,
        &[control],
        Some((active_policy_document, true)),
    );
    assert_policy_read_model(&policies[1], document_archived_policy, &[control], None);
}
