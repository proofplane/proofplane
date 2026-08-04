use http::{header::SET_COOKIE, StatusCode};
use proofplane::{domain::WorkspacePermission, routes::request_context::REQUEST_ID_HEADER};
use serde_json::json;
use url::Url;

use crate::support::{
    auditor_access::invite_token, harness, mcp::McpClient, oauth::authorize_agent_connection,
    scenario::ScenarioBuilder,
};

use super::helpers::{
    assert_auth_failure_audit, assert_authentication_rejected_page,
    assert_authentication_unavailable_page,
};

#[tokio::test]
async fn provider_rejection_and_unavailability_return_stable_coarse_outcomes() {
    let app = harness::app().await;
    let owner_subject = "auth0|auditor-provider-failure-owner";
    let auditor_email = "auditor-provider-failure@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner_subject)
        .with_workspace(owner_subject, "Auditor Provider Failure")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Provider Failure").id;
    let token = authorize_agent_connection(
        &app,
        owner_subject,
        "Auditor Provider Failure Agent",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let created = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2026-04-01T00:00:00Z",
                "period_end": "2026-06-30T23:59:59Z",
            }),
        )
        .await;
    let invitation_token = invite_token(
        &Url::parse(created["url"].as_str().expect("auditor URL is text"))
            .expect("auditor URL parses"),
    );

    let rejected_start = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/login"))
        .form(&[("token", invitation_token.as_str())])
        .await;
    rejected_start.assert_status(StatusCode::SEE_OTHER);
    let rejected_state = Url::parse(
        rejected_start
            .header("location")
            .to_str()
            .expect("authorization location is text"),
    )
    .expect("authorization URL parses")
    .query_pairs()
    .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
    .expect("authorization URL carries state");
    let rejected_code = "auditor-provider-rejected-code";
    let _rejected = app.auditor_identity_provider().rejected(rejected_code);
    let ((rejected, rejected_request_id), rejected_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get("/auditor-access/auth0/callback")
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .add_query_param("code", rejected_code)
                .add_query_param("state", &rejected_state)
                .await;
            (response, request_id)
        })
        .await;
    rejected.assert_status_bad_request();
    assert_authentication_rejected_page(&rejected.text());
    assert_eq!(rejected.headers().get_all(SET_COOKIE).iter().count(), 0);
    assert_eq!(rejected_logs.len(), 1);
    assert_auth_failure_audit(&rejected_logs[0], "rejected", rejected_request_id);
    assert_eq!(
        app.auditor_identity_provider()
            .exchanges_for(rejected_code)
            .len(),
        1
    );

    let unavailable_start = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/login"))
        .form(&[("token", invitation_token.as_str())])
        .await;
    unavailable_start.assert_status(StatusCode::SEE_OTHER);
    let unavailable_state = Url::parse(
        unavailable_start
            .header("location")
            .to_str()
            .expect("authorization location is text"),
    )
    .expect("authorization URL parses")
    .query_pairs()
    .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
    .expect("authorization URL carries state");
    let unavailable_code = "auditor-provider-unavailable-code";
    let _unavailable = app
        .auditor_identity_provider()
        .unavailable(unavailable_code);
    let ((unavailable, unavailable_request_id), unavailable_logs) = app
        .capture_audit_logs(async |request_id| {
            let response = app
                .app_server()
                .get("/auditor-access/auth0/callback")
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .add_query_param("code", unavailable_code)
                .add_query_param("state", &unavailable_state)
                .await;
            (response, request_id)
        })
        .await;
    unavailable.assert_status(StatusCode::SERVICE_UNAVAILABLE);
    assert_authentication_unavailable_page(&unavailable.text());
    assert_eq!(unavailable.headers().get_all(SET_COOKIE).iter().count(), 0);
    assert_eq!(unavailable_logs.len(), 1);
    assert_auth_failure_audit(
        &unavailable_logs[0],
        "provider_unavailable",
        unavailable_request_id,
    );
    assert_eq!(
        app.auditor_identity_provider()
            .exchanges_for(unavailable_code)
            .len(),
        1
    );
}

#[tokio::test]
async fn unregistered_authorization_codes_are_rejected_by_default() {
    let app = harness::app().await;
    let owner_subject = "auth0|auditor-unregistered-code-owner";
    let auditor_email = "auditor-unregistered-code@example.com";
    let authorization_code = "auditor-unregistered-code";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner_subject)
        .with_workspace(owner_subject, "Auditor Unregistered Code")
        .build()
        .await;
    let workspace_id = scenario.workspace("Auditor Unregistered Code").id;
    let token = authorize_agent_connection(
        &app,
        owner_subject,
        "Auditor Unregistered Code Agent",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let created = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": "2099-01-01T00:00:00Z",
                "period_start": "2026-07-01T00:00:00Z",
                "period_end": "2026-09-30T23:59:59Z",
            }),
        )
        .await;
    let invitation_token = invite_token(
        &Url::parse(created["url"].as_str().expect("auditor URL is text"))
            .expect("auditor URL parses"),
    );
    let started = app
        .app_server()
        .post(&format!("/auditor-access/{workspace_id}/login"))
        .form(&[("token", invitation_token.as_str())])
        .await;
    started.assert_status(StatusCode::SEE_OTHER);
    let state = Url::parse(
        started
            .header("location")
            .to_str()
            .expect("authorization location is text"),
    )
    .expect("authorization URL parses")
    .query_pairs()
    .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
    .expect("authorization URL carries state");

    let response = app
        .app_server()
        .get("/auditor-access/auth0/callback")
        .add_query_param("code", authorization_code)
        .add_query_param("state", &state)
        .await;
    response.assert_status_bad_request();
    assert_authentication_rejected_page(&response.text());
    assert_eq!(response.headers().get_all(SET_COOKIE).iter().count(), 0);
    assert_eq!(
        app.auditor_identity_provider()
            .exchanges_for(authorization_code)
            .len(),
        1
    );
}
