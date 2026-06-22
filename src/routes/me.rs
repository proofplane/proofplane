use axum::{
    extract::{Request, State},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    authentication::{auth0::TokenVerifier, UserAuthenticator, UserContext},
    domain::User,
    observability::audit::{AuditActor, AuditClientType, AuditEvent, AuditObject, AuditOutcome},
    routes::{authentication::authenticate_user, error::ApiError, request_context::RequestId},
    services::user::UserService,
};

pub struct MeState<V: TokenVerifier> {
    pub service: UserService,
    pub route_auth: UserRouteAuthState<V>,
}

impl<V: TokenVerifier> Clone for MeState<V> {
    fn clone(&self) -> Self {
        Self {
            service: self.service.clone(),
            route_auth: self.route_auth.clone(),
        }
    }
}

pub struct UserRouteAuthState<V: TokenVerifier> {
    pub authenticator: UserAuthenticator<V>,
}

impl<V: TokenVerifier> Clone for UserRouteAuthState<V> {
    fn clone(&self) -> Self {
        Self {
            authenticator: self.authenticator.clone(),
        }
    }
}

pub fn router<V: TokenVerifier + 'static>(state: MeState<V>) -> Router {
    let route_auth = state.route_auth.clone();

    Router::new()
        .route("/me", get(get_me::<V>))
        .route("/login", post(login::<V>))
        .route_layer(middleware::from_fn_with_state(
            route_auth,
            authenticate_user_route::<V>,
        ))
        .with_state(state)
}

async fn authenticate_user_route<V: TokenVerifier>(
    State(state): State<UserRouteAuthState<V>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    authenticate_user(&state.authenticator, &mut request).await?;

    Ok(next.run(request).await)
}

async fn get_me<V: TokenVerifier>(
    State(state): State<MeState<V>>,
    Extension(user): Extension<UserContext>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = state
        .service
        .get_user(user.user_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(user.into()))
}

async fn login<V: TokenVerifier>(
    State(state): State<MeState<V>>,
    Extension(user): Extension<UserContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = state
        .service
        .record_login(user.user_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    AuditEvent::new(
        "user.logged_in",
        AuditOutcome::Success,
        AuditActor::User {
            user_id: user.id.into(),
        },
        AuditClientType::Rest,
        "login",
    )
    .request_id(request_id.0)
    .object(AuditObject::new("user", user.id.into()))
    .emit();

    Ok(Json(user.into()))
}

#[derive(Debug, Serialize)]
struct UserResponse {
    id: Uuid,
    auth0_sub: String,
    email: Option<String>,
    name: Option<String>,
    last_login_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id.into(),
            auth0_sub: user.auth0_sub,
            email: user.email,
            name: user.name,
            last_login_at: user.last_login_at,
            created_at: user.created_at,
        }
    }
}
