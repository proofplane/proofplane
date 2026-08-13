use crate::support::{harness, scenario::ScenarioBuilder};
use http::StatusCode;
use proofplane::routes::{
    authentication::AUTHORIZATION_HEADER, request_context::REQUEST_ID_HEADER,
};
use serde_json::{json, Value};

#[tokio::test]
async fn administrator_lists_copies_and_revokes_current_invitation_without_exposing_authority() {
    let app = harness::app().await;
    let owner = "auth0|pending-management-owner";
    let administrator = "auth0|pending-management-admin";
    let foreign_owner = "auth0|pending-management-foreign";
    let scenario = ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_user(administrator)
        .with_user(foreign_owner)
        .with_workspace(owner, "Pending Management Workspace")
        .with_workspace(foreign_owner, "Pending Management Foreign")
        .build()
        .await;

    let administrator_invitation = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "email": format!("{administrator}@example.com") }))
        .await;
    administrator_invitation.assert_status_ok();
    let administrator_invitation: Value = administrator_invitation.json();
    let administrator_token = invitation_token(&administrator_invitation["url"]);
    app.app_server()
        .post("/workspace-invitations/accept")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {administrator}"))
        .json(&json!({ "token": administrator_token }))
        .await
        .assert_status_ok();

    let created = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {administrator}"))
        .json(&json!({ "email": "managed-invitation@example.com" }))
        .await;
    created.assert_status_ok();
    let created: Value = created.json();
    let invitation_id = created["id"].as_str().unwrap();
    let original_generation = created["generation"].clone();
    let original_expiry = created["expires_at"].clone();
    let original_token = invitation_token(&created["url"]);

    let people = app
        .app_server()
        .get("/workspace/people")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {administrator}"))
        .await;
    people.assert_status_ok();
    let people: Value = people.json();
    assert_eq!(people["actor_role"], "admin");
    assert_eq!(people["pending_invitations"].as_array().unwrap().len(), 1);
    let pending = &people["pending_invitations"][0];
    assert_eq!(
        keys(pending),
        [
            "delivered_at",
            "delivery_failed_at",
            "delivery_state",
            "expires_at",
            "generation",
            "id",
            "invited_email",
            "queued_at",
            "role"
        ]
    );
    assert_eq!(pending["id"], invitation_id);
    assert_eq!(pending["invited_email"], "managed-invitation@example.com");
    assert_eq!(pending["role"], "admin");
    assert_eq!(pending["generation"], original_generation);
    assert_eq!(pending["expires_at"], original_expiry);
    assert!(matches!(
        pending["delivery_state"].as_str(),
        Some("queued" | "delivered")
    ));

    let first_copy = app
        .app_server()
        .post(&format!("/workspace/invitations/{invitation_id}/link"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {administrator}"))
        .await;
    first_copy.assert_status_ok();
    let first_copy: Value = first_copy.json();
    let second_copy = app
        .app_server()
        .post(&format!("/workspace/invitations/{invitation_id}/link"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {administrator}"))
        .await;
    second_copy.assert_status_ok();
    let second_copy: Value = second_copy.json();
    assert_eq!(first_copy["generation"], original_generation);
    assert_eq!(second_copy["generation"], original_generation);
    assert_eq!(first_copy["expires_at"], original_expiry);
    assert_eq!(second_copy["expires_at"], original_expiry);
    for token in [
        original_token,
        invitation_token(&first_copy["url"]),
        invitation_token(&second_copy["url"]),
    ] {
        app.app_server()
            .post("/workspace-invitations/preview")
            .json(&json!({ "token": token }))
            .await
            .assert_status_ok();
    }

    let stale = app
        .app_server()
        .post(&format!("/workspace/invitations/{invitation_id}/resend"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "expected_generation": original_generation }))
        .await;
    stale.assert_status_ok();
    let rotated: Value = stale.json();
    assert_eq!(rotated["generation"], 2);
    let rotated_token = invitation_token(&rotated["url"]);

    let stale_revoke = app
        .app_server()
        .delete(&format!("/workspace/invitations/{invitation_id}"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {administrator}"))
        .json(&json!({ "expected_generation": 1 }))
        .await;
    assert_eq!(stale_revoke.status_code(), StatusCode::CONFLICT);
    assert_eq!(
        stale_revoke.json::<Value>(),
        json!({
            "error": {
                "code": "stale_invitation_generation",
                "message": "the invitation generation has changed",
                "details": []
            }
        })
    );

    for actor in [foreign_owner, "auth0|pending-management-outsider"] {
        app.app_server()
            .delete(&format!("/workspace/invitations/{invitation_id}"))
            .add_header(AUTHORIZATION_HEADER, format!("Bearer {actor}"))
            .json(&json!({ "expected_generation": 2 }))
            .await
            .assert_status_not_found();
    }

    let (revoked, logs) = app
        .capture_audit_logs(async |request_id| {
            app.app_server()
                .delete(&format!("/workspace/invitations/{invitation_id}"))
                .add_header(AUTHORIZATION_HEADER, format!("Bearer {administrator}"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .json(&json!({ "expected_generation": 2 }))
                .await
        })
        .await;
    revoked.assert_status_ok();
    assert_eq!(
        revoked.json::<Value>(),
        json!({ "id": invitation_id, "status": "revoked" })
    );
    assert_eq!(logs.len(), 1);
    let fields = &logs[0]["fields"];
    assert_eq!(fields["event_name"], "workspace.invitation_revoked");
    assert_eq!(fields["outcome"], "success");
    assert_eq!(fields["operation"], "revoke_workspace_invitation");
    assert_eq!(
        fields["user_id"],
        scenario.user(administrator).id.to_string()
    );
    assert_eq!(fields["object_type"], "workspace_invitation");
    assert_eq!(fields["object_id"], invitation_id);
    assert_eq!(
        fields["workspace_id"],
        scenario
            .workspace("Pending Management Workspace")
            .id
            .to_string()
    );

    let people = app
        .app_server()
        .get("/workspace/people")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .await;
    people.assert_status_ok();
    assert_eq!(people.json::<Value>()["pending_invitations"], json!([]));
    app.app_server()
        .post("/workspace-invitations/preview")
        .json(&json!({ "token": rotated_token }))
        .await
        .assert_status(StatusCode::GONE);
    app.app_server()
        .post(&format!("/workspace/invitations/{invitation_id}/link"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .await
        .assert_status_not_found();
    let (replay, replay_logs) = app
        .capture_audit_logs(async |request_id| {
            app.app_server()
                .delete(&format!("/workspace/invitations/{invitation_id}"))
                .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
                .add_header(REQUEST_ID_HEADER, request_id.to_string())
                .json(&json!({ "expected_generation": 2 }))
                .await
        })
        .await;
    replay.assert_status_not_found();
    assert!(replay_logs.is_empty());

    let accepted_revoke = app
        .app_server()
        .delete(&format!(
            "/workspace/invitations/{}",
            administrator_invitation["id"].as_str().unwrap()
        ))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "expected_generation": 1 }))
        .await;
    accepted_revoke.assert_status_not_found();
}

fn invitation_token(url: &Value) -> String {
    url::Url::parse(url.as_str().unwrap())
        .unwrap()
        .fragment()
        .unwrap()
        .strip_prefix("token=")
        .unwrap()
        .to_owned()
}

fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}
