use chrono::{Duration, Utc};
use proofplane::domain::{
    ActorId, ActorKind, CreateActorPayload, CreateApiCredentialPayload, CreateWorkspacePayload,
    UpdateActorPayload, UpdateApiCredentialPayload, UpdateWorkspacePayload,
};
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn actor_repository_crud_uses_typed_rows() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = postgres
        .create_actor(&CreateActorPayload {
            id: Some(ActorId::from(Uuid::new_v4())),
            kind: ActorKind::HumanUser,
            display_name: "Repository Human".to_owned(),
        })
        .await
        .expect("actor creates");

    assert_eq!(
        postgres
            .get_actor(actor.id)
            .await
            .expect("actor reads")
            .expect("actor exists"),
        actor
    );
    assert!(postgres
        .list_actors()
        .await
        .expect("actors list")
        .contains(&actor));

    let updated = postgres
        .update_actor(
            actor.id,
            &UpdateActorPayload {
                kind: ActorKind::ServiceAccount,
                display_name: "Repository Service".to_owned(),
            },
        )
        .await
        .expect("actor updates")
        .expect("actor exists");
    assert_eq!(updated.kind, ActorKind::ServiceAccount);
    assert_eq!(updated.display_name, "Repository Service");
    assert!(postgres
        .list_actors()
        .await
        .expect("actors list")
        .contains(&updated));

    assert!(postgres
        .update_actor(
            ActorId::from(Uuid::new_v4()),
            &UpdateActorPayload {
                kind: ActorKind::System,
                display_name: "Missing".to_owned(),
            },
        )
        .await
        .expect("missing actor update resolves")
        .is_none());
    assert!(postgres
        .delete_actor(actor.id)
        .await
        .expect("actor deletes"));
    assert!(postgres
        .get_actor(actor.id)
        .await
        .expect("deleted actor reads")
        .is_none());
    assert!(!postgres
        .delete_actor(actor.id)
        .await
        .expect("second actor delete resolves"));
}

#[tokio::test]
async fn workspace_repository_crud_uses_typed_rows() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let workspace = postgres
        .create_workspace(&CreateWorkspacePayload {
            id: None,
            slug: Some("repository-workspace".to_owned()),
            name: "Repository Workspace".to_owned(),
        })
        .await
        .expect("workspace creates");

    assert_eq!(
        postgres
            .get_workspace(workspace.id)
            .await
            .expect("workspace reads")
            .expect("workspace exists"),
        workspace
    );

    let updated = postgres
        .update_workspace(
            workspace.id,
            &UpdateWorkspacePayload {
                slug: None,
                name: "Renamed Workspace".to_owned(),
            },
        )
        .await
        .expect("workspace updates")
        .expect("workspace exists");
    assert_eq!(updated.slug, None);
    assert_eq!(updated.name, "Renamed Workspace");
    assert!(postgres
        .list_workspaces()
        .await
        .expect("workspaces list")
        .contains(&updated));

    assert!(postgres
        .update_workspace(
            Uuid::new_v4().into(),
            &UpdateWorkspacePayload {
                slug: Some("missing-workspace".to_owned()),
                name: "Missing Workspace".to_owned(),
            },
        )
        .await
        .expect("missing workspace update resolves")
        .is_none());
    assert!(postgres
        .delete_workspace(workspace.id)
        .await
        .expect("workspace deletes"));
    assert!(postgres
        .get_workspace(workspace.id)
        .await
        .expect("deleted workspace reads")
        .is_none());
    assert!(!postgres
        .delete_workspace(workspace.id)
        .await
        .expect("second workspace delete resolves"));
}

#[tokio::test]
async fn api_credential_repository_crud_uses_lifecycle_fields() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = postgres
        .create_actor(&CreateActorPayload {
            id: None,
            kind: ActorKind::Integration,
            display_name: "Credential Actor".to_owned(),
        })
        .await
        .expect("credential actor creates");
    let credential = postgres
        .create_api_credential(&CreateApiCredentialPayload {
            id: "repository-api-key".to_owned(),
            actor_id: actor.id,
            name: "Repository API Key".to_owned(),
            key_id: "first-key-id".to_owned(),
            credential_hash: "first-credential-hash".to_owned(),
            expires_at: Some(Utc::now() + Duration::days(1)),
            revoked_at: None,
        })
        .await
        .expect("API credential creates");

    assert_eq!(
        postgres
            .get_api_credential(&credential.id)
            .await
            .expect("API credential reads")
            .expect("API credential exists"),
        credential
    );

    let updated = postgres
        .update_api_credential(
            &credential.id,
            &UpdateApiCredentialPayload {
                name: "Rotated Repository API Key".to_owned(),
                key_id: "rotated-key-id".to_owned(),
                credential_hash: "rotated-credential-hash".to_owned(),
                expires_at: None,
                revoked_at: Some(Utc::now()),
            },
        )
        .await
        .expect("API credential updates")
        .expect("API credential exists");
    assert_eq!(updated.credential_hash, "rotated-credential-hash");
    assert_eq!(updated.key_id, "rotated-key-id");
    assert!(updated.expires_at.is_none());
    assert!(updated.revoked_at.is_some());
    let actor_with_credential = postgres
        .actor_with_api_credential(actor.id)
        .await
        .expect("actor credential reads")
        .expect("actor exists");
    assert_eq!(actor_with_credential.actor, actor);
    assert_eq!(actor_with_credential.api_credential, updated.clone());
    assert!(postgres
        .list_api_credentials()
        .await
        .expect("API credentials list")
        .contains(&updated));

    assert!(postgres
        .update_api_credential(
            "missing-api-key",
            &UpdateApiCredentialPayload {
                name: "Missing API Key".to_owned(),
                key_id: "missing-key-id".to_owned(),
                credential_hash: "missing-credential-hash".to_owned(),
                expires_at: None,
                revoked_at: None,
            },
        )
        .await
        .expect("missing API credential update resolves")
        .is_none());
    assert!(postgres
        .delete_api_credential(&credential.id)
        .await
        .expect("API credential deletes"));
    assert!(postgres
        .get_api_credential(&credential.id)
        .await
        .expect("deleted API credential reads")
        .is_none());
    assert!(!postgres
        .delete_api_credential(&credential.id)
        .await
        .expect("second API credential delete resolves"));
    assert!(postgres
        .delete_actor(actor.id)
        .await
        .expect("credential actor deletes"));
}

#[tokio::test]
async fn api_credential_repository_enforces_one_credential_per_actor() {
    let app = TestApp::start().await;
    let postgres = app.postgres();
    let actor = postgres
        .create_actor(&CreateActorPayload {
            id: None,
            kind: ActorKind::Integration,
            display_name: "Single Credential Actor".to_owned(),
        })
        .await
        .expect("credential actor creates");

    for (id, key_id) in [
        ("first-api-key", "first-key-id"),
        ("second-api-key", "second-key-id"),
    ] {
        let result = postgres
            .create_api_credential(&CreateApiCredentialPayload {
                id: id.to_owned(),
                actor_id: actor.id,
                name: id.to_owned(),
                key_id: key_id.to_owned(),
                credential_hash: format!("{id}-hash"),
                expires_at: None,
                revoked_at: None,
            })
            .await;

        if id == "first-api-key" {
            result.expect("first API credential creates");
        } else {
            result.expect_err("second API credential violates actor constraint");
        }
    }
}
