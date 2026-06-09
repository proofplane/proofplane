use std::sync::Arc;

use axum::{
    extract::{Request, State},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    authentication::UserAuthenticator,
    domain::User,
    repository::Postgres,
    routes::{
        authentication::{authenticate_user, UserContext},
        error::ApiError,
    },
};

#[derive(Clone)]
pub struct MeState {
    pub repository: Arc<Postgres>,
    pub route_auth: UserRouteAuthState,
}

#[derive(Clone)]
pub struct UserRouteAuthState {
    pub authenticator: UserAuthenticator,
}

pub fn router(state: MeState) -> Router {
    let route_auth = state.route_auth.clone();

    Router::new()
        .route("/me", get(get_me))
        .route_layer(middleware::from_fn_with_state(
            route_auth,
            authenticate_user_route,
        ))
        .with_state(state)
}

async fn authenticate_user_route(
    State(state): State<UserRouteAuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    authenticate_user(&state.authenticator, &mut request).await?;

    Ok(next.run(request).await)
}

async fn get_me(
    State(state): State<MeState>,
    Extension(user): Extension<UserContext>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = state
        .repository
        .get_user(user.user_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to load authenticated user");
            ApiError::Internal
        })?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(user.into()))
}

#[derive(Debug, Serialize)]
struct UserResponse {
    id: Uuid,
    auth0_sub: String,
    email: Option<String>,
    name: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id.into(),
            auth0_sub: user.auth0_sub,
            email: user.email,
            name: user.name,
            created_at: user.created_at,
        }
    }
}
