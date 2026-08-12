use http::StatusCode;
use proofplane::routes::authentication::AUTHORIZATION_HEADER;
use serde_json::{json, Value};

use crate::support::{harness, scenario::ScenarioBuilder};

#[tokio::test]
async fn copyable_invitation_previews_accepts_and_replays_without_leaking_authority() {
    let app = harness::app().await;
    let owner = "auth0|invitation-owner";
    let invitee = "auth0|invitation-invitee";
    ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_user(invitee)
        .with_workspace(owner, "Invitation Workspace")
        .build()
        .await;

    let invited_email = format!("{invitee}@example.com");
    let created = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "email": invited_email }))
        .await;
    created.assert_status_ok();
    let created: Value = created.json();
    assert_eq!(
        keys(&created),
        [
            "delivery_state",
            "expires_at",
            "generation",
            "id",
            "invited_email",
            "role",
            "url"
        ]
    );
    assert_eq!(created["role"], "admin");
    assert_eq!(created["generation"], 1);
    assert_eq!(created["delivery_state"], "not_queued");
    let link = url::Url::parse(created["url"].as_str().unwrap()).unwrap();
    assert_eq!(link.path(), "/join");
    assert!(link.query().is_none());
    let token = link.fragment().unwrap().strip_prefix("token=").unwrap();

    let preview = app
        .app_server()
        .post("/workspace-invitations/preview")
        .json(&json!({ "token": token }))
        .await;
    preview.assert_status_ok();
    let preview: Value = preview.json();
    assert_eq!(
        keys(&preview),
        ["expires_at", "invited_email", "role", "workspace_name"]
    );
    assert_eq!(preview["workspace_name"], "Invitation Workspace");
    assert_eq!(preview["invited_email"], invited_email);

    let accepted = app
        .app_server()
        .post("/workspace-invitations/accept")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {invitee}"))
        .json(&json!({ "token": token }))
        .await;
    accepted.assert_status_ok();
    let accepted: Value = accepted.json();
    assert_eq!(accepted["role"], "admin");

    let replay = app
        .app_server()
        .post("/workspace-invitations/accept")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {invitee}"))
        .json(&json!({ "token": token }))
        .await;
    replay.assert_status_ok();
    assert_eq!(replay.json::<Value>(), accepted);

    let people = app
        .app_server()
        .get("/workspace/people")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .await;
    people.assert_status_ok();
    let people: Value = people.json();
    assert_eq!(
        keys(&people),
        ["actor_role", "members", "pending_invitations", "workspace"]
    );
    assert_eq!(people["actor_role"], "owner");
    assert_eq!(people["members"].as_array().unwrap().len(), 2);
    assert_eq!(people["pending_invitations"], json!([]));
}

#[tokio::test]
async fn duplicate_and_cross_workspace_link_requests_are_stable_and_concealed() {
    let app = harness::app().await;
    let owner = "auth0|invitation-duplicate-owner";
    let foreign_owner = "auth0|invitation-foreign-owner";
    ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_user(foreign_owner)
        .with_workspace(owner, "Duplicate Workspace")
        .with_workspace(foreign_owner, "Foreign Workspace")
        .build()
        .await;
    let email = "future-admin@example.com";

    let created = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "email": email }))
        .await;
    created.assert_status_ok();
    let created: Value = created.json();

    let duplicate = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "email": " Future-Admin@Example.com " }))
        .await;
    assert_eq!(duplicate.status_code(), StatusCode::CONFLICT);
    let duplicate: Value = duplicate.json();
    assert_eq!(duplicate["error"]["code"], "invitation_already_pending");
    assert_eq!(duplicate["invitation"]["id"], created["id"]);
    assert_eq!(duplicate["invitation"]["generation"], created["generation"]);
    assert_eq!(
        keys(&duplicate["invitation"]),
        ["expires_at", "generation", "id", "invited_email", "role"]
    );

    let concealed = app
        .app_server()
        .post(&format!(
            "/workspace/invitations/{}/link",
            created["id"].as_str().unwrap()
        ))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {foreign_owner}"))
        .await;
    assert_eq!(concealed.status_code(), StatusCode::NOT_FOUND);

    let current = app
        .app_server()
        .post(&format!(
            "/workspace/invitations/{}/link",
            created["id"].as_str().unwrap()
        ))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .await;
    current.assert_status_ok();
    let current: Value = current.json();
    assert_eq!(current["generation"], created["generation"]);
    assert_eq!(current["expires_at"], created["expires_at"]);
}

#[tokio::test]
async fn concurrent_acceptance_by_the_same_verified_user_has_two_stable_successes() {
    let app = harness::app().await;
    let owner = "auth0|invitation-concurrent-owner";
    let invitee = "auth0|invitation-concurrent-invitee";
    ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_user(invitee)
        .with_workspace(owner, "Concurrent Invitation Workspace")
        .build()
        .await;
    let created = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "email": format!("{invitee}@example.com") }))
        .await;
    created.assert_status_ok();
    let created: Value = created.json();
    let link = url::Url::parse(created["url"].as_str().unwrap()).unwrap();
    let token = link.fragment().unwrap().strip_prefix("token=").unwrap();

    let first = app
        .app_server()
        .post("/workspace-invitations/accept")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {invitee}"))
        .json(&json!({ "token": token }));
    let second = app
        .app_server()
        .post("/workspace-invitations/accept")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {invitee}"))
        .json(&json!({ "token": token }));
    let (first, second) = tokio::join!(first, second);
    first.assert_status_ok();
    second.assert_status_ok();
    assert_eq!(first.json::<Value>(), second.json::<Value>());
}

fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect()
}
