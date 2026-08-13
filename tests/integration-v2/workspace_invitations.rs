use http::StatusCode;
use proofplane::routes::authentication::AUTHORIZATION_HEADER;
use proofplane::{
    messaging::SEND_WORKSPACE_INVITATION_TYPE, routes::request_context::REQUEST_ID_HEADER,
};
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
    assert_eq!(created["delivery_state"], "queued");
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
async fn creation_delivers_and_resend_rotates_authority_with_stable_conflict() {
    let app = harness::app().await;
    let owner = "auth0|invitation-delivery-owner";
    ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_workspace(owner, "Delivery Workspace")
        .build()
        .await;
    let request_id = uuid::Uuid::new_v4();
    let mut events = app.pipeline_events().subscribe();
    let created = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .add_header(REQUEST_ID_HEADER, request_id.to_string())
        .json(&json!({ "email": "delivery-admin@example.com" }))
        .await;
    created.assert_status_ok();
    let created: Value = created.json();
    let invitation_id = created["id"].as_str().unwrap();
    assert_eq!(
        events
            .await_event(SEND_WORKSPACE_INVITATION_TYPE, invitation_id)
            .await,
        StatusCode::NO_CONTENT
    );
    let messages = app.mail_messages();
    let delivered = messages
        .iter()
        .find(|message| message.to == "delivery-admin@example.com")
        .unwrap();
    assert_eq!(delivered.to, "delivery-admin@example.com");
    assert_eq!(
        delivered.idempotency_key,
        format!("workspace-invitation/{invitation_id}/1")
    );
    let delivered_url = delivered
        .text
        .split_whitespace()
        .find(|value| value.starts_with("https://app.proofplane.test/join#token="))
        .unwrap()
        .to_owned();
    let delivered_url_parsed = url::Url::parse(&delivered_url).unwrap();
    let delivered_token = delivered_url_parsed
        .fragment()
        .unwrap()
        .strip_prefix("token=")
        .unwrap();
    let delivered_preview = app
        .app_server()
        .post("/workspace-invitations/preview")
        .json(&json!({ "token": delivered_token }))
        .await;
    delivered_preview.assert_status_ok();

    let old_token = url::Url::parse(created["url"].as_str().unwrap())
        .unwrap()
        .fragment()
        .unwrap()
        .strip_prefix("token=")
        .unwrap()
        .to_owned();
    let resend_request_id = uuid::Uuid::new_v4();
    let resent = app
        .app_server()
        .post(&format!("/workspace/invitations/{invitation_id}/resend"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .add_header(REQUEST_ID_HEADER, resend_request_id.to_string())
        .json(&json!({ "expected_generation": 1 }))
        .await;
    resent.assert_status_ok();
    let resent: Value = resent.json();
    assert_eq!(resent["generation"], 2);
    assert_eq!(resent["delivery_state"], "queued");
    assert_ne!(resent["url"], created["url"]);

    let stale = app
        .app_server()
        .post(&format!("/workspace/invitations/{invitation_id}/resend"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "expected_generation": 1 }))
        .await;
    assert_eq!(stale.status_code(), StatusCode::CONFLICT);
    assert_eq!(
        stale.json::<Value>()["error"]["code"],
        "stale_invitation_generation"
    );

    let old_preview = app
        .app_server()
        .post("/workspace-invitations/preview")
        .json(&json!({ "token": old_token }))
        .await;
    assert_eq!(old_preview.status_code(), StatusCode::GONE);
    assert_eq!(
        events
            .await_event(SEND_WORKSPACE_INVITATION_TYPE, invitation_id)
            .await,
        StatusCode::NO_CONTENT
    );
    let messages = app.mail_messages();
    let resent_mail = messages
        .iter()
        .filter(|message| message.to == "delivery-admin@example.com")
        .find(|message| message.idempotency_key.ends_with("/2"))
        .unwrap();
    assert_eq!(
        resent_mail.idempotency_key,
        format!("workspace-invitation/{invitation_id}/2")
    );
    let resent_url = resent_mail
        .text
        .split_whitespace()
        .find(|value| value.starts_with("https://app.proofplane.test/join#token="))
        .unwrap();
    assert_ne!(resent_url, delivered_url);
}

#[tokio::test]
async fn duplicate_worker_delivery_uses_provider_idempotency_key_and_acks() {
    let app = harness::app().await;
    let owner = "auth0|invitation-idempotency-owner";
    ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_workspace(owner, "Idempotency Workspace")
        .build()
        .await;
    let request_id = uuid::Uuid::new_v4();
    let mut failure = app
        .pipeline_controls()
        .fail_after_forward_once(SEND_WORKSPACE_INVITATION_TYPE, request_id);
    let created = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .add_header(REQUEST_ID_HEADER, request_id.to_string())
        .json(&json!({ "email": "idempotent-admin@example.com" }))
        .await;
    created.assert_status_ok();
    let first = failure.await_first_delivery().await;
    let deliveries = failure.await_redelivery().await;
    failure.release();
    assert_eq!(first, deliveries[0]);
    assert_eq!(deliveries[0].message_id, deliveries[1].message_id);
    assert_eq!(deliveries[0].worker_status, StatusCode::NO_CONTENT);
    assert_eq!(deliveries[1].worker_status, StatusCode::NO_CONTENT);
    let messages: Vec<_> = app
        .mail_messages()
        .into_iter()
        .filter(|message| message.to == "idempotent-admin@example.com")
        .collect();
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages.last().unwrap().idempotency_key,
        format!(
            "workspace-invitation/{}/1",
            created.json::<Value>()["id"].as_str().unwrap()
        )
    );
}

#[tokio::test]
async fn stale_generation_worker_command_acks_without_sending_wrong_generation() {
    let app = harness::app().await;
    let owner = "auth0|invitation-stale-worker-owner";
    ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_workspace(owner, "Stale Worker Workspace")
        .build()
        .await;
    let request_id = uuid::Uuid::new_v4();
    let mut gate = app
        .pipeline_controls()
        .hold(SEND_WORKSPACE_INVITATION_TYPE, request_id);
    let created = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .add_header(REQUEST_ID_HEADER, request_id.to_string())
        .json(&json!({ "email": "stale-worker-admin@example.com" }))
        .await;
    created.assert_status_ok();
    let created: Value = created.json();
    let invitation_id = created["id"].as_str().unwrap();
    let interception = gate.await_interception().await;
    assert_eq!(interception.aggregate_id, invitation_id);

    let mut events = app.pipeline_events().subscribe();
    let resent = app
        .app_server()
        .post(&format!("/workspace/invitations/{invitation_id}/resend"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "expected_generation": 1 }))
        .await;
    resent.assert_status_ok();
    gate.release();
    assert_eq!(
        events
            .await_event(SEND_WORKSPACE_INVITATION_TYPE, invitation_id)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        events
            .await_event(SEND_WORKSPACE_INVITATION_TYPE, invitation_id)
            .await,
        StatusCode::NO_CONTENT
    );
    let messages: Vec<_> = app
        .mail_messages()
        .into_iter()
        .filter(|message| message.to == "stale-worker-admin@example.com")
        .collect();
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].idempotency_key,
        format!("workspace-invitation/{invitation_id}/2")
    );
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
