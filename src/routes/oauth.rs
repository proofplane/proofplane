use std::{str::FromStr, sync::Arc};

use axum::{
    extract::{Form, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::{
    authentication::{
        auth0::TokenVerifier,
        paseto::{OAuthTokenIssuer, OAuthTokenVerifier, RegisteredClaims},
        UserAuthenticator, UserContext,
    },
    config::PasetoOAuthConfig,
    domain::{WorkspacePermission, WorkspacePermissions},
    repository::Postgres,
    routes::{authentication::authenticate_user, error::ApiError},
};

const CODE_TTL: Duration = Duration::minutes(5);
const ACCESS_TTL: Duration = Duration::minutes(15);
const REFRESH_IDLE_TTL: Duration = Duration::days(30);
const REFRESH_ABSOLUTE_TTL: Duration = Duration::days(90);

pub struct OAuthState<V: TokenVerifier> {
    pub issuer: Url,
    pub app: Url,
    pub resource: Url,
    pub postgres: Arc<Postgres>,
    pub user_authenticator: UserAuthenticator<V>,
    issuer_impl: Arc<OAuthTokenIssuer>,
    verifier: OAuthTokenVerifier,
}

impl<V: TokenVerifier> Clone for OAuthState<V> {
    fn clone(&self) -> Self {
        Self {
            issuer: self.issuer.clone(),
            app: self.app.clone(),
            resource: self.resource.clone(),
            postgres: self.postgres.clone(),
            user_authenticator: self.user_authenticator.clone(),
            issuer_impl: self.issuer_impl.clone(),
            verifier: self.verifier.clone(),
        }
    }
}

impl<V: TokenVerifier> OAuthState<V> {
    pub fn new(
        issuer: Url,
        app: Url,
        resource: Url,
        postgres: Arc<Postgres>,
        user_authenticator: UserAuthenticator<V>,
        keys: &PasetoOAuthConfig,
    ) -> Result<Self, crate::authentication::paseto::Error> {
        Ok(Self {
            issuer_impl: Arc::new(OAuthTokenIssuer::from_config(issuer.clone(), keys)?),
            verifier: OAuthTokenVerifier::from_config(issuer.clone(), keys)?,
            issuer,
            app,
            resource,
            postgres,
            user_authenticator,
        })
    }
}

pub fn router<V: TokenVerifier + 'static>(state: OAuthState<V>) -> Router {
    let protected = Router::new()
        .route("/oauth/requests/{id}", get(inspect::<V>))
        .route("/oauth/requests/{id}/approve", post(approve::<V>))
        .route("/oauth/requests/{id}/deny", post(deny::<V>))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            authenticate::<V>,
        ));
    Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(metadata::<V>),
        )
        .route("/oauth/authorize", get(authorize::<V>))
        .route("/oauth/token", post(token::<V>))
        .route("/oauth/revoke", post(revoke::<V>))
        .merge(protected)
        .with_state(state)
}

async fn authenticate<V: TokenVerifier>(
    State(state): State<OAuthState<V>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    authenticate_user(&state.user_authenticator, &mut request).await?;
    Ok(next.run(request).await)
}

async fn metadata<V: TokenVerifier>(State(state): State<OAuthState<V>>) -> Json<Metadata> {
    Json(Metadata {
        issuer: state.issuer.to_string(),
        authorization_endpoint: state.issuer.join("oauth/authorize").unwrap().to_string(),
        token_endpoint: state.issuer.join("oauth/token").unwrap().to_string(),
        revocation_endpoint: state.issuer.join("oauth/revoke").unwrap().to_string(),
        response_types_supported: ["code"],
        grant_types_supported: ["authorization_code", "refresh_token"],
        code_challenge_methods_supported: ["S256"],
        scopes_supported: WorkspacePermission::ALL.map(WorkspacePermission::as_str),
    })
}

async fn authorize<V: TokenVerifier>(
    State(state): State<OAuthState<V>>,
    Query(query): Query<AuthorizeQuery>,
) -> Result<Redirect, OAuthError> {
    if query.response_type != "code"
        || query.code_challenge_method != "S256"
        || query.resource != state.resource.as_str()
        || query.code_challenge.len() != 43
        || parse_scopes(&query.scope).is_none()
    {
        return Err(OAuthError);
    }
    let client = state.postgres.get().await.map_err(|_| OAuthError)?;
    let known = client
        .query_opt(
            "SELECT 1 FROM oauth_clients WHERE id = $1 AND $2 = ANY(redirect_uris)",
            &[&query.client_id, &query.redirect_uri],
        )
        .await
        .map_err(|_| OAuthError)?
        .is_some();
    if !known {
        return Err(OAuthError);
    }
    let id = Uuid::new_v4();
    let scopes = query.scope.split_ascii_whitespace().collect::<Vec<_>>();
    client
        .execute(
            "INSERT INTO oauth_authorization_requests
             (id, client_id, redirect_uri, resource, scope, state, code_challenge, expires_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
            &[
                &id,
                &query.client_id,
                &query.redirect_uri,
                &query.resource,
                &scopes,
                &query.state,
                &query.code_challenge,
                &(Utc::now() + CODE_TTL),
            ],
        )
        .await
        .map_err(|_| OAuthError)?;
    let mut location = state
        .app
        .join("connect/mcp/authorize")
        .map_err(|_| OAuthError)?;
    location
        .query_pairs_mut()
        .append_pair("request_id", &id.to_string());
    Ok(Redirect::to(location.as_str()))
}

async fn inspect<V: TokenVerifier>(
    State(state): State<OAuthState<V>>,
    Extension(_user): Extension<UserContext>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<RequestView>, OAuthError> {
    let client = state.postgres.get().await.map_err(|_| OAuthError)?;
    let row = client
        .query_opt(
            "SELECT c.name, r.scope, r.expires_at
             FROM oauth_authorization_requests r JOIN oauth_clients c ON c.id=r.client_id
             WHERE r.id=$1 AND r.decided_at IS NULL AND r.expires_at > now()",
            &[&id],
        )
        .await
        .map_err(|_| OAuthError)?
        .ok_or(OAuthError)?;
    Ok(Json(RequestView {
        id,
        client_name: row.get(0),
        scopes: row.get(1),
        expires_at: row.get(2),
    }))
}

async fn approve<V: TokenVerifier>(
    State(state): State<OAuthState<V>>,
    Extension(user): Extension<UserContext>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Json(body): Json<ApproveRequest>,
) -> Result<Json<Decision>, OAuthError> {
    if state
        .postgres
        .get_membership_role(body.workspace_id.into(), user.user_id)
        .await
        .map_err(|_| OAuthError)?
        .is_none()
    {
        return Err(OAuthError);
    }
    let mut client = state.postgres.get().await.map_err(|_| OAuthError)?;
    let tx = client.transaction().await.map_err(|_| OAuthError)?;
    let row = tx
        .query_opt(
            "SELECT client_id, redirect_uri, resource, scope, state, code_challenge
             FROM oauth_authorization_requests
             WHERE id=$1 AND decided_at IS NULL AND expires_at > now() FOR UPDATE",
            &[&id],
        )
        .await
        .map_err(|_| OAuthError)?
        .ok_or(OAuthError)?;
    let scopes: Vec<String> = row.get("scope");
    let permissions = scopes
        .iter()
        .filter(|scope| scope.as_str() != "offline_access")
        .cloned()
        .collect::<Vec<_>>();
    if permissions.is_empty()
        || permissions
            .iter()
            .any(|scope| WorkspacePermission::from_str(scope).is_err())
    {
        return Err(OAuthError);
    }
    let connection_id = Uuid::new_v4();
    tx.execute(
        "INSERT INTO agent_connections (id,user_id,workspace_id,client_id,resource,permissions)
         VALUES ($1,$2,$3,$4,$5,$6)",
        &[
            &connection_id,
            &Uuid::from(user.user_id),
            &body.workspace_id,
            &row.get::<_, String>("client_id"),
            &row.get::<_, String>("resource"),
            &permissions,
        ],
    )
    .await
    .map_err(|_| OAuthError)?;
    // ponytail: existing data-plane transactions require an api_tokens principal;
    // this internal, unparseable row avoids duplicating every repository path.
    tx.execute(
        "INSERT INTO api_tokens (id,digest,user_id,workspace_id,name,expires_at)
         VALUES ($1,$2,$3,$4,'OAuth agent connection',$5)",
        &[
            &connection_id,
            &digest(&connection_id.to_string()),
            &Uuid::from(user.user_id),
            &body.workspace_id,
            &(Utc::now() + REFRESH_ABSOLUTE_TTL),
        ],
    )
    .await
    .map_err(|_| OAuthError)?;
    for permission in &permissions {
        tx.execute(
            "INSERT INTO api_token_permissions (api_token_id,permission) VALUES ($1,$2)",
            &[&connection_id, permission],
        )
        .await
        .map_err(|_| OAuthError)?;
    }
    let code = random_secret()?;
    tx.execute(
        "INSERT INTO oauth_authorization_codes (digest,request_id,connection_id,expires_at)
         VALUES ($1,$2,$3,$4)",
        &[
            &digest(&code),
            &id,
            &connection_id,
            &(Utc::now() + CODE_TTL),
        ],
    )
    .await
    .map_err(|_| OAuthError)?;
    tx.execute(
        "UPDATE oauth_authorization_requests SET user_id=$2,workspace_id=$3,approved=true,decided_at=now() WHERE id=$1",
        &[&id, &Uuid::from(user.user_id), &body.workspace_id],
    ).await.map_err(|_| OAuthError)?;
    let redirect_uri: String = row.get("redirect_uri");
    let oauth_state: Option<String> = row.get("state");
    tx.commit().await.map_err(|_| OAuthError)?;
    Ok(Json(Decision {
        redirect_uri: callback(
            &redirect_uri,
            &[("code", code), ("state", oauth_state.unwrap_or_default())],
        )?,
    }))
}

async fn deny<V: TokenVerifier>(
    State(state): State<OAuthState<V>>,
    Extension(_user): Extension<UserContext>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Decision>, OAuthError> {
    let client = state.postgres.get().await.map_err(|_| OAuthError)?;
    let row = client
        .query_opt(
            "UPDATE oauth_authorization_requests SET approved=false,decided_at=now()
         WHERE id=$1 AND decided_at IS NULL AND expires_at > now()
         RETURNING redirect_uri,state",
            &[&id],
        )
        .await
        .map_err(|_| OAuthError)?
        .ok_or(OAuthError)?;
    Ok(Json(Decision {
        redirect_uri: callback(
            row.get(0),
            &[
                ("error", "access_denied".to_owned()),
                ("state", row.get::<_, Option<String>>(1).unwrap_or_default()),
            ],
        )?,
    }))
}

async fn token<V: TokenVerifier>(
    State(state): State<OAuthState<V>>,
    Form(form): Form<TokenRequest>,
) -> Result<Json<TokenResponse>, OAuthError> {
    match form.grant_type.as_str() {
        "authorization_code" => exchange_code(&state, form).await,
        "refresh_token" => exchange_refresh(&state, form.refresh_token.ok_or(OAuthError)?).await,
        _ => Err(OAuthError),
    }
}

async fn exchange_code<V: TokenVerifier>(
    state: &OAuthState<V>,
    form: TokenRequest,
) -> Result<Json<TokenResponse>, OAuthError> {
    let code = form.code.ok_or(OAuthError)?;
    let verifier = form.code_verifier.ok_or(OAuthError)?;
    let mut client = state.postgres.get().await.map_err(|_| OAuthError)?;
    let tx = client.transaction().await.map_err(|_| OAuthError)?;
    let row = tx
        .query_opt(
            "SELECT c.connection_id,r.client_id,r.redirect_uri,r.resource,r.scope,r.code_challenge,
                a.user_id,a.workspace_id,a.permissions
         FROM oauth_authorization_codes c
         JOIN oauth_authorization_requests r ON r.id=c.request_id
         JOIN agent_connections a ON a.id=c.connection_id
         WHERE c.digest=$1 AND c.used_at IS NULL AND c.expires_at > now() FOR UPDATE",
            &[&digest(&code)],
        )
        .await
        .map_err(|_| OAuthError)?
        .ok_or(OAuthError)?;
    if form.client_id.as_deref() != Some(row.get::<_, String>("client_id").as_str())
        || form.redirect_uri.as_deref() != Some(row.get::<_, String>("redirect_uri").as_str())
        || form.resource.as_deref() != Some(row.get::<_, String>("resource").as_str())
        || pkce(&verifier) != row.get::<_, String>("code_challenge")
    {
        return Err(OAuthError);
    }
    tx.execute(
        "UPDATE oauth_authorization_codes SET used_at=now() WHERE digest=$1",
        &[&digest(&code)],
    )
    .await
    .map_err(|_| OAuthError)?;
    let tokens = issue_tokens(
        state,
        &tx,
        &row,
        row.get::<_, Vec<String>>("scope")
            .iter()
            .any(|s| s == "offline_access"),
        None,
        None,
    )
    .await?;
    tx.commit().await.map_err(|_| OAuthError)?;
    Ok(Json(tokens))
}

async fn exchange_refresh<V: TokenVerifier>(
    state: &OAuthState<V>,
    raw: String,
) -> Result<Json<TokenResponse>, OAuthError> {
    let claims = state
        .verifier
        .verify::<TokenClaims>(&raw, state.resource.as_str(), true)
        .map_err(|_| OAuthError)?
        .claims;
    let mut client = state.postgres.get().await.map_err(|_| OAuthError)?;
    let tx = client.transaction().await.map_err(|_| OAuthError)?;
    let row = tx.query_opt(
        "SELECT t.used_at,t.family_id,t.absolute_expires_at,a.id connection_id,a.user_id,a.workspace_id,a.client_id,a.resource,a.permissions
         FROM oauth_refresh_tokens t JOIN agent_connections a ON a.id=t.connection_id
         WHERE t.digest=$1 AND t.revoked_at IS NULL AND t.expires_at > now()
           AND t.absolute_expires_at > now() AND a.revoked_at IS NULL FOR UPDATE",
        &[&digest(&raw)]
    ).await.map_err(|_| OAuthError)?.ok_or(OAuthError)?;
    if row
        .get::<_, Option<chrono::DateTime<Utc>>>("used_at")
        .is_some()
    {
        tx.execute(
            "UPDATE agent_connections SET revoked_at=now() WHERE id=$1",
            &[&claims.connection_id],
        )
        .await
        .map_err(|_| OAuthError)?;
        tx.execute(
            "UPDATE oauth_refresh_tokens SET revoked_at=now() WHERE family_id=$1",
            &[&claims.family_id],
        )
        .await
        .map_err(|_| OAuthError)?;
        tx.commit().await.map_err(|_| OAuthError)?;
        return Err(OAuthError);
    }
    tx.execute(
        "UPDATE oauth_refresh_tokens SET used_at=now() WHERE digest=$1",
        &[&digest(&raw)],
    )
    .await
    .map_err(|_| OAuthError)?;
    let tokens = issue_tokens(
        state,
        &tx,
        &row,
        true,
        Some(claims.family_id),
        Some(row.get("absolute_expires_at")),
    )
    .await?;
    tx.commit().await.map_err(|_| OAuthError)?;
    Ok(Json(tokens))
}

async fn issue_tokens<V: TokenVerifier>(
    state: &OAuthState<V>,
    tx: &tokio_postgres::Transaction<'_>,
    row: &tokio_postgres::Row,
    refresh: bool,
    family: Option<Uuid>,
    absolute_expiry: Option<chrono::DateTime<Utc>>,
) -> Result<TokenResponse, OAuthError> {
    let now = Utc::now();
    let claims = TokenClaims {
        connection_id: row.get("connection_id"),
        user_id: row.get("user_id"),
        workspace_id: row.get("workspace_id"),
        client_id: row.get("client_id"),
        resource: row.get("resource"),
        permissions: row.get("permissions"),
        family_id: family.unwrap_or_else(Uuid::new_v4),
    };
    let access = state
        .issuer_impl
        .issue(
            state.resource.as_str(),
            RegisteredClaims {
                subject: claims.connection_id,
                token_id: Uuid::new_v4(),
                expires_at: now + ACCESS_TTL,
            },
            &claims,
            false,
        )
        .map_err(|_| OAuthError)?;
    let refresh_token = if refresh {
        let absolute = absolute_expiry.unwrap_or(now + REFRESH_ABSOLUTE_TTL);
        let issued = state
            .issuer_impl
            .issue(
                state.resource.as_str(),
                RegisteredClaims {
                    subject: claims.connection_id,
                    token_id: Uuid::new_v4(),
                    expires_at: now + REFRESH_IDLE_TTL,
                },
                &claims,
                true,
            )
            .map_err(|_| OAuthError)?;
        tx.execute(
            "INSERT INTO oauth_refresh_tokens (digest,family_id,connection_id,expires_at,absolute_expires_at)
             VALUES ($1,$2,$3,$4,$5)",
            &[&digest(&issued.token), &claims.family_id, &claims.connection_id, &issued.expires_at, &absolute],
        ).await.map_err(|_| OAuthError)?;
        Some(issued.token)
    } else {
        None
    };
    Ok(TokenResponse {
        access_token: access.token,
        token_type: "Bearer",
        expires_in: 900,
        refresh_token,
        scope: claims.permissions.join(" "),
    })
}

async fn revoke<V: TokenVerifier>(
    State(state): State<OAuthState<V>>,
    Form(form): Form<RevokeRequest>,
) -> StatusCode {
    let verified = state
        .verifier
        .verify::<TokenClaims>(&form.token, state.resource.as_str(), true)
        .or_else(|_| {
            state
                .verifier
                .verify::<TokenClaims>(&form.token, state.resource.as_str(), false)
        });
    if let Ok(token) = verified {
        if let Ok(client) = state.postgres.get().await {
            let _ = client
                .execute(
                    "UPDATE agent_connections SET revoked_at=now() WHERE id=$1",
                    &[&token.claims.connection_id],
                )
                .await;
            let _ = client
                .execute(
                    "UPDATE oauth_refresh_tokens SET revoked_at=now() WHERE family_id=$1",
                    &[&token.claims.family_id],
                )
                .await;
        }
    }
    StatusCode::OK
}

fn parse_scopes(value: &str) -> Option<WorkspacePermissions> {
    let mut result = WorkspacePermissions::none();
    for scope in value.split_ascii_whitespace() {
        if scope == "offline_access" {
            continue;
        }
        result.insert(WorkspacePermission::from_str(scope).ok()?);
    }
    (!result.is_empty()).then_some(result)
}

fn random_secret() -> Result<String, OAuthError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| OAuthError)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn digest(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}
fn pkce(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}
fn callback(base: &str, pairs: &[(&str, String)]) -> Result<String, OAuthError> {
    let mut url = Url::parse(base).map_err(|_| OAuthError)?;
    for (key, value) in pairs {
        if !value.is_empty() {
            url.query_pairs_mut().append_pair(key, value);
        }
    }
    Ok(url.into())
}

#[derive(Debug)]
struct OAuthError;
impl IntoResponse for OAuthError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":"invalid_grant"})),
        )
            .into_response()
    }
}
#[derive(Deserialize)]
struct AuthorizeQuery {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    resource: String,
    scope: String,
    state: Option<String>,
    code_challenge: String,
    code_challenge_method: String,
}
#[derive(Deserialize)]
struct ApproveRequest {
    workspace_id: Uuid,
}
#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    client_id: Option<String>,
    code_verifier: Option<String>,
    resource: Option<String>,
    refresh_token: Option<String>,
}
#[derive(Deserialize)]
struct RevokeRequest {
    token: String,
}
#[derive(Serialize)]
struct Decision {
    redirect_uri: String,
}
#[derive(Serialize)]
struct RequestView {
    id: Uuid,
    client_name: String,
    scopes: Vec<String>,
    expires_at: chrono::DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    pub connection_id: Uuid,
    pub user_id: Uuid,
    pub workspace_id: Uuid,
    pub client_id: String,
    pub resource: String,
    pub permissions: Vec<String>,
    pub family_id: Uuid,
}
#[derive(Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    scope: String,
}
#[derive(Serialize)]
struct Metadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    revocation_endpoint: String,
    response_types_supported: [&'static str; 1],
    grant_types_supported: [&'static str; 2],
    code_challenge_methods_supported: [&'static str; 1],
    scopes_supported: [&'static str; 6],
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pkce_and_scopes_are_strict() {
        assert_eq!(
            pkce("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(parse_scopes("read_controls offline_access")
            .unwrap()
            .has(WorkspacePermission::ReadControls));
        assert!(parse_scopes("unknown").is_none());
    }
}
