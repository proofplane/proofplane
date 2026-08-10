use super::{assertions::*, *};

#[tokio::test]
async fn initial_invite_enters_hosted_login_and_empty_portal_lists_seeded_requirements() {
    let app = harness::app().await;
    let subject = "auth0|auditor-browser-initial";
    let workspace_name = "Auditor Browser Initial";
    let auditor_email = "auditor-browser-initial@example.com";

    let scenario = ScenarioBuilder::new(&app)
        .with_user(subject)
        .with_workspace(subject, workspace_name)
        .build()
        .await;
    let workspace_id = scenario.workspace(workspace_name).id;
    let soc2 = scenario.framework("soc2");

    let token = authorize_agent_connection(
        &app,
        subject,
        "Auditor Browser Initial Access",
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
    let invite_url = Url::parse(created["url"].as_str().expect("invite URL is text"))
        .expect("invite URL parses");
    let invite_token = invite_token(&invite_url);

    let invite = app.app_server().get(&local_path(invite_url.as_str())).await;
    invite.assert_status_ok();
    assert_eq!(
        body_read_model(&invite.text()),
        invite_body(workspace_id, &invite_token, auditor_email)
    );

    let auditor_cookie = authenticate_auditor(
        &app,
        workspace_id,
        &invite_token,
        "auth0|auditor-browser-initial-identity",
        auditor_email,
    )
    .await;

    let portal = app
        .app_server()
        .get("/auditor-access/portal")
        .add_header("cookie", auditor_cookie)
        .await;
    portal.assert_status_ok();
    let rows = [
        requirement_row(
            soc2.requirement("CC6.1"),
            0,
            0,
            0,
            "gap",
            "No controls mapped",
        ),
        requirement_row(
            soc2.requirement("CC7.1"),
            0,
            0,
            0,
            "gap",
            "No controls mapped",
        ),
    ]
    .join("");
    assert_eq!(
        body_read_model(&portal.text()),
        portal_body(workspace_name, auditor_email, 2, 0, &rows)
    );
}
