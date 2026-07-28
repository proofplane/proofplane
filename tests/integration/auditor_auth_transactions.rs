use std::collections::HashMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use proofplane::{
    config::Auth0AuditorPortalConfig,
    domain::{AuditReviewPeriod, Sha256Digest},
    services::{
        auditor_access_grants::{
            AuditorAccessGrantService, CreateAuditorAccessGrantRequest, IssuedAuditorAccessGrant,
        },
        auditor_auth_transactions::{AuditorAuthTransactionError, AuditorAuthTransactionService},
    },
};
use secrecy::ExposeSecret;
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use super::support::TestApp;

#[tokio::test]
async fn authorization_start_persists_only_digests_and_builds_exact_auth0_request() {
    let app = auth_transaction_app().await;
    let grant = create_grant(&app).await;
    let service = transaction_service(&app);
    let before = Utc::now();

    let start = service.start(&grant.grant).await.expect("start creates");
    let after = Utc::now();
    let parameters = query_parameters(start.redirect_url());
    let state = parameters.get("state").expect("state is present");
    let nonce = parameters.get("nonce").expect("nonce is present");

    assert_eq!(
        start.redirect_url().as_str().split('?').next(),
        Some("https://proofplane-test.us.auth0.com/authorize")
    );
    assert_eq!(
        parameters.get("client_id").map(String::as_str),
        Some("auditor-client")
    );
    assert_eq!(
        parameters.get("redirect_uri").map(String::as_str),
        Some("https://api.proofplane.test/auditor-access/auth0/callback")
    );
    assert_eq!(
        parameters.get("response_type").map(String::as_str),
        Some("code")
    );
    assert_eq!(
        parameters.get("scope").map(String::as_str),
        Some("openid email")
    );
    assert_eq!(
        parameters.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    assert_eq!(
        parameters.get("connection").map(String::as_str),
        Some("email")
    );
    assert_eq!(
        parameters.get("login_hint").map(String::as_str),
        Some("auditor@example.com")
    );
    assert_eq!(parameters.get("prompt").map(String::as_str), Some("login"));
    assert_eq!(state.len(), 43);
    assert_eq!(nonce.len(), 43);
    assert_ne!(state, nonce);

    let row = app
        .postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            r#"
SELECT state_digest, nonce_digest, pkce_verifier, expires_at
FROM auditor_auth_transactions
WHERE id = $1
"#,
            &[&Uuid::from(start.transaction_id)],
        )
        .await
        .expect("transaction reads");
    let state_digest: Vec<u8> = row.get("state_digest");
    let nonce_digest: Vec<u8> = row.get("nonce_digest");
    let verifier: String = row.get("pkce_verifier");
    let expires_at: DateTime<Utc> = row.get("expires_at");

    assert_eq!(state_digest, Sha256::digest(state.as_bytes()).as_slice());
    assert_eq!(nonce_digest, Sha256::digest(nonce.as_bytes()).as_slice());
    assert_eq!(state_digest.len(), 32);
    assert_eq!(nonce_digest.len(), 32);
    assert_eq!(verifier.len(), 43);
    let expected_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    assert_eq!(
        parameters.get("code_challenge").map(String::as_str),
        Some(expected_challenge.as_str())
    );
    assert!(!start.redirect_url().as_str().contains(&verifier));
    assert!(expires_at >= before + Duration::minutes(10));
    assert!(expires_at <= after + Duration::minutes(10));

    let debug = format!("{start:?}");
    assert!(!debug.contains(state));
    assert!(!debug.contains(nonce));
    assert!(!debug.contains(&verifier));
}

#[tokio::test]
async fn claim_is_atomic_one_use_and_returns_the_authoritative_grant_binding() {
    let app = auth_transaction_app().await;
    let grant = create_grant(&app).await;
    let service = transaction_service(&app);
    let start = service.start(&grant.grant).await.expect("start creates");
    let parameters = query_parameters(start.redirect_url());
    let state = parameters.get("state").expect("state is present").clone();
    let nonce = parameters.get("nonce").expect("nonce is present").clone();

    let (left, right) = tokio::join!(service.claim(&state), service.claim(&state));
    let successes = [&left, &right]
        .into_iter()
        .filter(|result| result.is_ok())
        .count();
    assert_eq!(successes, 1);
    let claimed = left.or(right).expect("one claim succeeds");
    assert_eq!(claimed.grant_id, grant.grant.id);
    assert_eq!(claimed.nonce_digest, Sha256Digest::digest(nonce.as_bytes()));
    assert_eq!(claimed.pkce_verifier.expose_secret().len(), 43);
    assert!(claimed.consumed_at < claimed.expires_at);
    assert!(matches!(
        service.claim(&state).await,
        Err(AuditorAuthTransactionError::Unavailable)
    ));
    assert!(matches!(
        service.claim("").await,
        Err(AuditorAuthTransactionError::Unavailable)
    ));

    let debug = format!("{claimed:?}");
    assert!(!debug.contains(claimed.pkce_verifier.expose_secret()));
}

#[tokio::test]
async fn expired_unknown_and_inactive_grant_transactions_are_rejected_without_material() {
    let app = auth_transaction_app().await;
    let grant = create_grant(&app).await;
    let service = transaction_service(&app);
    let start = service.start(&grant.grant).await.expect("start creates");
    let state = query_parameters(start.redirect_url())
        .get("state")
        .expect("state is present")
        .clone();

    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE auditor_auth_transactions SET expires_at = now() - interval '1 second' WHERE id = $1",
            &[&Uuid::from(start.transaction_id)],
        )
        .await
        .expect("transaction expires");
    assert!(matches!(
        service.claim(&state).await,
        Err(AuditorAuthTransactionError::Unavailable)
    ));
    assert!(matches!(
        service.claim("unknown-state").await,
        Err(AuditorAuthTransactionError::Unavailable)
    ));

    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE auditor_access_grants SET revoked_at = now() WHERE id = $1",
            &[&Uuid::from(grant.grant.id)],
        )
        .await
        .expect("grant revokes");
    assert!(matches!(
        service.start(&grant.grant).await,
        Err(AuditorAuthTransactionError::Unavailable)
    ));

    let expired_grant = create_grant(&app).await;
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE auditor_access_grants SET created_at = now() - interval '2 seconds', expires_at = now() - interval '1 second' WHERE id = $1",
            &[&Uuid::from(expired_grant.grant.id)],
        )
        .await
        .expect("grant expires");
    assert!(matches!(
        service.start(&expired_grant.grant).await,
        Err(AuditorAuthTransactionError::Unavailable)
    ));

    let grants = AuditorAccessGrantService::new(app.postgres_arc());
    assert!(grants
        .load_for_use(app.workspace_id("workspace").into(), "invalid-invitation")
        .await
        .is_err());
    assert_eq!(total_transaction_count(&app).await, 0);
}

#[tokio::test]
async fn starts_keep_grants_separate_and_cleanup_only_stale_rows_for_the_same_grant() {
    let app = auth_transaction_app().await;
    let first_grant = create_grant(&app).await;
    let second_grant = create_grant(&app).await;
    let service = transaction_service(&app);

    let consumed = service
        .start(&first_grant.grant)
        .await
        .expect("first start creates");
    let consumed_state = query_parameters(consumed.redirect_url())
        .get("state")
        .expect("state is present")
        .clone();
    service
        .claim(&consumed_state)
        .await
        .expect("first transaction claims");

    let active = service
        .start(&first_grant.grant)
        .await
        .expect("replacement creates");
    assert_eq!(
        transaction_count(&app, first_grant.grant.id.into()).await,
        1
    );

    let other = service
        .start(&second_grant.grant)
        .await
        .expect("other grant start creates");
    let other_state = query_parameters(other.redirect_url())
        .get("state")
        .expect("state is present")
        .clone();
    let claimed_other = service
        .claim(&other_state)
        .await
        .expect("other state claims");
    assert_eq!(claimed_other.grant_id, second_grant.grant.id);
    assert_eq!(
        transaction_count(&app, first_grant.grant.id.into()).await,
        1
    );

    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .execute(
            "UPDATE auditor_auth_transactions SET expires_at = now() - interval '1 second' WHERE id = $1",
            &[&Uuid::from(active.transaction_id)],
        )
        .await
        .expect("active transaction expires");
    service
        .start(&first_grant.grant)
        .await
        .expect("expired transaction is replaced");
    assert_eq!(
        transaction_count(&app, first_grant.grant.id.into()).await,
        1
    );
    assert_eq!(
        transaction_count(&app, second_grant.grant.id.into()).await,
        1
    );
}

fn transaction_service(app: &TestApp) -> AuditorAuthTransactionService {
    AuditorAuthTransactionService::new(app.postgres_arc(), auditor_auth0_config())
}

fn auditor_auth0_config() -> Auth0AuditorPortalConfig {
    Auth0AuditorPortalConfig {
        client_id: "auditor-client".to_owned(),
        client_secret: "auditor-secret".to_owned().into(),
        callback_path: "/auditor-access/auth0/callback".to_owned(),
        callback_url: Url::parse("https://api.proofplane.test/auditor-access/auth0/callback")
            .expect("callback URL parses"),
        connection: "email".to_owned(),
        authorization_endpoint: Url::parse("https://proofplane-test.us.auth0.com/authorize")
            .expect("authorization endpoint parses"),
        token_endpoint: Url::parse("https://proofplane-test.us.auth0.com/oauth/token")
            .expect("token endpoint parses"),
    }
}

async fn auth_transaction_app() -> TestApp {
    TestApp::builder()
        .without_default_auth()
        .workspace("workspace", "Auditor auth transaction workspace")
        .with_default_membership()
        .build()
        .await
}

async fn create_grant(app: &TestApp) -> IssuedAuditorAccessGrant {
    let workspace_id = app.workspace_id("workspace");
    AuditorAccessGrantService::new(app.postgres_arc())
        .create(
            &app.agent_connection_context(workspace_id),
            CreateAuditorAccessGrantRequest {
                auditor_email: " Auditor@Example.COM ".to_owned(),
                expires_at: None,
                period: AuditReviewPeriod::new(Utc::now() - Duration::days(90), Utc::now())
                    .expect("review period is valid"),
            },
        )
        .await
        .expect("auditor grant creates")
}

fn query_parameters(url: &Url) -> HashMap<String, String> {
    url.query_pairs().into_owned().collect()
}

async fn transaction_count(app: &TestApp, grant_id: Uuid) -> i64 {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT count(*)::bigint AS count FROM auditor_auth_transactions WHERE grant_id = $1",
            &[&grant_id],
        )
        .await
        .expect("transaction count reads")
        .get("count")
}

async fn total_transaction_count(app: &TestApp) -> i64 {
    app.postgres()
        .get()
        .await
        .expect("connection opens")
        .query_one(
            "SELECT count(*)::bigint AS count FROM auditor_auth_transactions",
            &[],
        )
        .await
        .expect("transaction count reads")
        .get("count")
}
