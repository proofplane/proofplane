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
    authentication::{auth0::TokenVerifier, UserAuthenticator, UserContext},
    domain::User,
    routes::{authentication::authenticate_user, error::ApiError},
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
