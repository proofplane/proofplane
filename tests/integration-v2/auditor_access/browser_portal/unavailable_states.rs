use super::{assertions::*, *};

#[tokio::test]
async fn empty_catalog_and_every_unavailable_browser_resource_share_one_recovery_page() {
    let app = harness::app().await;
    let subject = "auth0|auditor-browser-unavailable";
    let workspace_name = "Auditor Browser Unavailable";
    let auditor_email = "auditor-browser-unavailable@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .build()
        .await;
    let workspace_id = scenario.workspace(workspace_name).id;

    let token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Browser Unavailable Access",
        &[WorkspacePermission::ManageAuditorAccess],
    )
    .await;
    let client = McpClient::connect(app.mcp_server(), &token).await;
    let created = client
        .call_tool(
            "create_auditor_access_link",
            json!({
                "email": auditor_email,
                "expires_at": EXPIRES_AT,
                "period_start": PERIOD_START,
                "period_end": PERIOD_END,
            }),
        )
        .await;
    let grant_id = created["grant"]["id"]
        .as_str()
        .expect("grant id is text")
        .to_owned();
    let invite_url = Url::parse(created["url"].as_str().expect("invite URL is text"))
        .expect("invite URL parses");
    let invite_token = invite_token(&invite_url);
    app.app_server()
        .get(&local_path(invite_url.as_str()))
        .await
        .assert_status_ok();
    app.app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/request/browser"
        ))
        .form(&[("token", invite_token.as_str())])
        .await
        .assert_status_ok();
    let sent = app.mailer().sent_mail_for(auditor_email);
    assert_eq!(sent.len(), 1);
    let code = sent[0].code.clone();
    let verified = app
        .app_server()
        .post(&format!(
            "/auditor-access/{workspace_id}/otp/verify/browser"
        ))
        .form(&[("token", invite_token.as_str()), ("code", code.as_str())])
        .await;
    verified.assert_status(StatusCode::SEE_OTHER);
    let auditor_cookie = request_cookie(
        verified
            .header("set-cookie")
            .to_str()
            .expect("auditor cookie is text"),
    );

    let empty_catalog = app
        .app_server()
        .get("/auditor-access/portal/policies")
        .add_header("cookie", auditor_cookie.clone())
        .await;
    empty_catalog.assert_status_ok();
    assert_eq!(
        body_projection(&empty_catalog.text()),
        empty_policies_body(workspace_name, auditor_email)
    );

    let unavailable = unavailable_body();
    let responses = [
        app.app_server()
            .get(&format!(
                "/auditor-access/{workspace_id}?token=not-an-invite-token"
            ))
            .await,
        app.app_server()
            .get("/auditor-access/portal/policies/not-a-uuid")
            .add_header("cookie", auditor_cookie.clone())
            .await,
        app.app_server()
            .get(&format!(
                "/auditor-access/portal/policies/{}",
                Uuid::new_v4()
            ))
            .add_header("cookie", auditor_cookie.clone())
            .await,
        app.app_server()
            .get(&format!(
                "/auditor-access/portal/controls/{}",
                Uuid::new_v4()
            ))
            .add_header("cookie", auditor_cookie.clone())
            .await,
        app.app_server()
            .get(&format!(
                "/auditor-access/portal/framework-requirements/{}",
                Uuid::new_v4()
            ))
            .add_header("cookie", auditor_cookie.clone())
            .await,
        app.app_server().get("/auditor-access/portal").await,
    ];
    for response in responses {
        response.assert_status_not_found();
        assert_eq!(body_projection(&response.text()), unavailable);
    }

    client
        .call_tool(
            "revoke_auditor_access_link",
            json!({ "grant_id": grant_id }),
        )
        .await;
    let revoked = app
        .app_server()
        .get("/auditor-access/portal")
        .add_header("cookie", auditor_cookie)
        .await;
    revoked.assert_status_not_found();
    assert_eq!(body_projection(&revoked.text()), unavailable);
}
