use chrono::{Duration, Utc};
use proofplane::{
    domain::{AuditReviewPeriod, WorkspacePermission, WorkspacePermissions},
    services::{
        agent_connections::AgentConnectionContext,
        auditor_access_grants::{
            AuditorAccessGrantError, AuditorAccessGrantService, CreateAuditorAccessGrantRequest,
        },
    },
};
use secrecy::ExposeSecret;
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn create_persists_metadata_and_digest_without_raw_secret() {
    let app = auditor_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let service = AuditorAccessGrantService::new(app.postgres_arc());

    let issued = service
        .create(
            &agent_connection_context(&app, workspace_id, WorkspacePermission::ALL),
            CreateAuditorAccessGrantRequest {
                auditor_email: " Auditor@Example.COM ".to_owned(),
                expires_at: None,
                period: AuditReviewPeriod::new(Utc::now() - Duration::days(90), Utc::now())
                    .expect("valid period"),
            },
        )
        .await
        .expect("auditor grant creates");
    let raw_secret = issued.raw_secret.expose_secret();

    assert_eq!(issued.grant.auditor_email, "auditor@example.com");
    assert_eq!(
        issued.grant.workspace_id.to_string(),
        workspace_id.to_string()
    );
    assert!(issued.grant.expires_at > Utc::now() + Duration::days(29));
    assert!(issued.grant.revoked_at.is_none());

    let row = app
        .postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            r#"
SELECT workspace_id, auditor_email, created_by_user_id, created_via_agent_connection_id,
       octet_length(secret_digest) AS digest_length, encode(secret_digest, 'hex') AS digest_hex
FROM auditor_access_grants
"#,
            &[],
        )
        .await
        .expect("grant row reads");
    assert_eq!(row.get::<_, Uuid>("workspace_id"), workspace_id);
    assert_eq!(row.get::<_, String>("auditor_email"), "auditor@example.com");
    assert_eq!(row.get::<_, Uuid>("created_by_user_id"), app.user_id());
    assert_eq!(
        row.get::<_, Uuid>("created_via_agent_connection_id"),
        app.api_token_id()
    );
    assert_eq!(row.get::<_, i32>("digest_length"), 32);
    assert!(!row.get::<_, String>("digest_hex").contains(raw_secret));
}

#[tokio::test]
async fn ordinary_read_token_cannot_create_grant() {
    let app = auditor_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let service = AuditorAccessGrantService::new(app.postgres_arc());

    let result = service
        .create(
            &agent_connection_context(&app, workspace_id, [WorkspacePermission::ReadEvidence]),
            CreateAuditorAccessGrantRequest {
                auditor_email: "auditor@example.com".to_owned(),
                expires_at: None,
                period: AuditReviewPeriod::new(Utc::now() - Duration::days(90), Utc::now())
                    .expect("valid period"),
            },
        )
        .await;

    assert!(matches!(result, Err(AuditorAccessGrantError::Denied)));
}

#[tokio::test]
async fn create_persists_audit_period() {
    let app = auditor_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let service = AuditorAccessGrantService::new(app.postgres_arc());
    let period_start = Utc::now() - Duration::days(90);
    let period_end = Utc::now();

    let issued = service
        .create(
            &agent_connection_context(&app, workspace_id, WorkspacePermission::ALL),
            CreateAuditorAccessGrantRequest {
                auditor_email: "auditor@example.com".to_owned(),
                expires_at: None,
                period: AuditReviewPeriod::new(period_start, period_end).expect("valid period"),
            },
        )
        .await
        .expect("auditor grant creates");

    // Round-tripped through TIMESTAMPTZ (microsecond precision), so compare loosely.
    assert!(
        (issued.grant.period.start - period_start)
            .num_seconds()
            .abs()
            < 1
    );
    assert!((issued.grant.period.end - period_end).num_seconds().abs() < 1);
    assert!(issued.grant.period.end >= issued.grant.period.start);
}

#[tokio::test]
async fn load_for_use_rejects_missing_revoked_expired_and_cross_workspace_grants() {
    let app = auditor_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let service = AuditorAccessGrantService::new(app.postgres_arc());

    assert_unavailable(
        service
            .load_for_use(workspace_id.into(), "not-a-secret")
            .await,
    );

    let issued = create_grant(&app, &service, workspace_id).await;
    assert_eq!(
        service
            .load_for_use(workspace_id.into(), issued.raw_secret.expose_secret())
            .await
            .expect("active grant loads")
            .id,
        issued.grant.id
    );
    assert_unavailable(
        service
            .load_for_use(other_workspace_id.into(), issued.raw_secret.expose_secret())
            .await,
    );

    service
        .revoke(
            &agent_connection_context(&app, workspace_id, WorkspacePermission::ALL),
            issued.grant.id,
        )
        .await
        .expect("grant revokes");
    assert_unavailable(
        service
            .load_for_use(workspace_id.into(), issued.raw_secret.expose_secret())
            .await,
    );

    let expired = create_grant(&app, &service, workspace_id).await;
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            r#"
UPDATE auditor_access_grants
SET created_at = now() - interval '40 days',
    expires_at = now() - interval '1 second'
WHERE id = $1
"#,
            &[&Uuid::from(expired.grant.id)],
        )
        .await
        .expect("grant expiry updates");
    assert_unavailable(
        service
            .load_for_use(workspace_id.into(), expired.raw_secret.expose_secret())
            .await,
    );
}

#[tokio::test]
async fn list_and_revoke_are_workspace_scoped() {
    let app = auditor_grant_app().await;
    let workspace_id = app.workspace_id("workspace");
    let other_workspace_id = app.workspace_id("other");
    let service = AuditorAccessGrantService::new(app.postgres_arc());
    let grant = create_grant(&app, &service, workspace_id).await.grant;
    let other_grant = create_grant(&app, &service, other_workspace_id).await.grant;

    let listed = service
        .list(&agent_connection_context(
            &app,
            workspace_id,
            WorkspacePermission::ALL,
        ))
        .await
        .expect("grants list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, grant.id);

    assert_unavailable(
        service
            .revoke(
                &agent_connection_context(&app, workspace_id, WorkspacePermission::ALL),
                other_grant.id,
            )
            .await,
    );
    assert!(service
        .revoke(
            &agent_connection_context(&app, workspace_id, WorkspacePermission::ALL),
            grant.id,
        )
        .await
        .expect("grant revokes")
        .revoked_at
        .is_some());
}

async fn auditor_grant_app() -> TestApp {
    TestApp::builder()
        .without_default_auth()
        .workspace("workspace", "Auditor grant workspace")
        .with_default_membership()
        .workspace("other", "Other auditor grant workspace")
        .with_default_membership()
        .build()
        .await
}

async fn create_grant(
    app: &TestApp,
    service: &AuditorAccessGrantService,
    workspace_id: Uuid,
) -> proofplane::services::auditor_access_grants::IssuedAuditorAccessGrant {
    service
        .create(
            &agent_connection_context(app, workspace_id, WorkspacePermission::ALL),
            CreateAuditorAccessGrantRequest {
                auditor_email: "auditor@example.com".to_owned(),
                expires_at: None,
                period: AuditReviewPeriod::new(Utc::now() - Duration::days(90), Utc::now())
                    .expect("valid period"),
            },
        )
        .await
        .expect("auditor grant creates")
}

fn agent_connection_context(
    app: &TestApp,
    workspace_id: Uuid,
    permissions: impl IntoIterator<Item = WorkspacePermission>,
) -> AgentConnectionContext {
    AgentConnectionContext {
        user_id: app.user_id().into(),
        connection_id: app.api_token_id().into(),
        workspace_id: workspace_id.into(),
        permissions: WorkspacePermissions::from_iter(permissions),
    }
}

fn assert_unavailable(result: Result<impl std::fmt::Debug, AuditorAccessGrantError>) {
    assert!(matches!(result, Err(AuditorAccessGrantError::Unavailable)));
}
