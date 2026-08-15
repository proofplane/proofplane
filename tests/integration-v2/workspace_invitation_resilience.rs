use http::StatusCode;
use proofplane::{
    mail::{MailError, MailFailureClass},
    messaging::SEND_WORKSPACE_INVITATION_TYPE,
    routes::authentication::AUTHORIZATION_HEADER,
};
use serde_json::{json, Value};

use crate::support::{harness, scenario::ScenarioBuilder};

#[tokio::test]
async fn mail_failures_retry_or_leave_the_copyable_invitation_manageable() {
    let app = harness::app().await;
    let owner = "auth0|invitation-mail-failure-owner";
    ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_workspace(owner, "Mail Failure Workspace")
        .build()
        .await;

    let retry_recipient = "retryable-invitation@example.com";
    app.fail_next_mail_for(
        retry_recipient,
        MailError {
            class: MailFailureClass::Retryable,
            status_class: "5xx",
        },
    );
    let mut events = app.pipeline_events().subscribe();
    let retried = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "email": retry_recipient }))
        .await;
    retried.assert_status_ok();
    let retried: Value = retried.json();
    let retried_id = retried["id"].as_str().unwrap();
    assert_eq!(
        events
            .await_event(SEND_WORKSPACE_INVITATION_TYPE, retried_id)
            .await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        events
            .await_event(SEND_WORKSPACE_INVITATION_TYPE, retried_id)
            .await,
        StatusCode::NO_CONTENT
    );
    let retried_messages: Vec<_> = app
        .mail_messages()
        .into_iter()
        .filter(|message| message.to == retry_recipient)
        .collect();
    assert_eq!(retried_messages.len(), 1);
    assert_eq!(
        retried_messages[0].idempotency_key,
        format!("workspace-invitation/{retried_id}/1")
    );

    let permanent_recipient = "permanent-invitation@example.com";
    app.fail_next_mail_for(
        permanent_recipient,
        MailError {
            class: MailFailureClass::Permanent,
            status_class: "4xx",
        },
    );
    let failed = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "email": permanent_recipient }))
        .await;
    failed.assert_status_ok();
    let failed: Value = failed.json();
    let failed_id = failed["id"].as_str().unwrap();
    assert_eq!(
        events
            .await_event(SEND_WORKSPACE_INVITATION_TYPE, failed_id)
            .await,
        StatusCode::NO_CONTENT
    );

    let people = app
        .app_server()
        .get("/workspace/people")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .await;
    people.assert_status_ok();
    let people: Value = people.json();
    let failed_invitation = people["pending_invitations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|invitation| invitation["id"] == failed_id)
        .unwrap();
    assert_eq!(failed_invitation["delivery_state"], "failed");

    let current_link = app
        .app_server()
        .post(&format!("/workspace/invitations/{failed_id}/link"))
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .await;
    current_link.assert_status_ok();
    assert_eq!(current_link.json::<Value>()["delivery_state"], "failed");
}

#[tokio::test]
async fn acceptance_rejects_wrong_unverified_and_already_tenanted_identities() {
    let app = harness::app().await;
    let owner = "auth0|invitation-rejection-owner";
    let invitee = "auth0|invitation-rejection-invitee";
    let wrong_user = "auth0|invitation-rejection-wrong";
    let tenanted_user = "auth0|invitation-rejection-tenanted";
    ScenarioBuilder::new(&app)
        .with_user(owner)
        .with_user(invitee)
        .with_user(wrong_user)
        .with_user(tenanted_user)
        .with_workspace(owner, "Invitation Rejection Workspace")
        .with_workspace(tenanted_user, "Existing Membership Workspace")
        .build()
        .await;

    let invitee_invitation = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "email": format!("{invitee}@example.com") }))
        .await;
    invitee_invitation.assert_status_ok();
    let invitee_invitation: Value = invitee_invitation.json();
    let invitee_token = token(&invitee_invitation);

    let wrong = app
        .app_server()
        .post("/workspace-invitations/accept")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {wrong_user}"))
        .json(&json!({ "token": invitee_token }))
        .await;
    assert_eq!(wrong.status_code(), StatusCode::FORBIDDEN);
    assert_eq!(
        wrong.json::<Value>()["error"]["code"],
        "invitation_email_mismatch"
    );

    let unverified = app
        .app_server()
        .post("/workspace-invitations/accept")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer noprofile:{invitee}"))
        .json(&json!({ "token": invitee_token }))
        .await;
    assert_eq!(unverified.status_code(), StatusCode::FORBIDDEN);
    assert_eq!(
        unverified.json::<Value>()["error"]["code"],
        "verified_email_required"
    );

    let tenanted_invitation = app
        .app_server()
        .post("/workspace/invitations")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
        .json(&json!({ "email": format!("{tenanted_user}@example.com") }))
        .await;
    tenanted_invitation.assert_status_ok();
    let tenanted_invitation: Value = tenanted_invitation.json();
    let existing_workspace = app
        .app_server()
        .post("/workspace-invitations/accept")
        .add_header(AUTHORIZATION_HEADER, format!("Bearer {tenanted_user}"))
        .json(&json!({ "token": token(&tenanted_invitation) }))
        .await;
    assert_eq!(existing_workspace.status_code(), StatusCode::CONFLICT);
    assert_eq!(
        existing_workspace.json::<Value>()["error"]["code"],
        "user_already_has_workspace"
    );
}

#[tokio::test]
async fn concurrent_duplicate_issuance_and_distinct_acceptance_have_one_winner() {
    let app = harness::app().await;
    let first_owner = "auth0|invitation-race-first-owner";
    let second_owner = "auth0|invitation-race-second-owner";
    let invitee = "auth0|invitation-race-invitee";
    ScenarioBuilder::new(&app)
        .with_user(first_owner)
        .with_user(second_owner)
        .with_user(invitee)
        .with_workspace(first_owner, "First Race Workspace")
        .with_workspace(second_owner, "Second Race Workspace")
        .build()
        .await;
    let email = format!("{invitee}@example.com");

    let create = |owner: &str| {
        app.app_server()
            .post("/workspace/invitations")
            .add_header(AUTHORIZATION_HEADER, format!("Bearer {owner}"))
            .json(&json!({ "email": email }))
    };
    let (first_a, first_b) = tokio::join!(create(first_owner), create(first_owner));
    let statuses = [first_a.status_code(), first_b.status_code()];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::OK)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::CONFLICT)
            .count(),
        1
    );
    let first_invitation: Value = if first_a.status_code() == StatusCode::OK {
        first_a.json()
    } else {
        first_b.json()
    };

    let second_invitation = create(second_owner).await;
    second_invitation.assert_status_ok();
    let second_invitation: Value = second_invitation.json();
    let first_token = token(&first_invitation);
    let second_token = token(&second_invitation);
    let accept = |token: String| {
        app.app_server()
            .post("/workspace-invitations/accept")
            .add_header(AUTHORIZATION_HEADER, format!("Bearer {invitee}"))
            .json(&json!({ "token": token }))
    };
    let (accepted_first, accepted_second) = tokio::join!(accept(first_token), accept(second_token));
    let acceptance_statuses = [accepted_first.status_code(), accepted_second.status_code()];
    assert_eq!(
        acceptance_statuses
            .iter()
            .filter(|status| **status == StatusCode::OK)
            .count(),
        1
    );
    assert_eq!(
        acceptance_statuses
            .iter()
            .filter(|status| **status == StatusCode::CONFLICT)
            .count(),
        1
    );
}

fn token(invitation: &Value) -> String {
    url::Url::parse(invitation["url"].as_str().unwrap())
        .unwrap()
        .fragment()
        .unwrap()
        .strip_prefix("token=")
        .unwrap()
        .to_owned()
}
