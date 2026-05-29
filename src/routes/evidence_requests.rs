use std::collections::HashMap;

use axum::{
    extract::{Path, Query, Request, State},
    http::Method,
    middleware,
    middleware::Next,
    response::Response,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::error;
use uuid::Uuid;

use crate::{
    authentication::ApiKeyAuthenticator,
    authorization::evidence_requests::EvidenceRequestAuthorizer,
    domain::{
        required_text, validate_freshness_window_days, CreateEvidenceRequestPayload, DomainError,
        EvidenceRequest, EvidenceRequestCadence, EvidenceRequestId, EvidenceRequestStatus,
        UpdateEvidenceRequestPayload, WorkspaceId,
    },
    routes::{
        authentication::authorize_workspace_route,
        error::{domain_errors, ApiError},
    },
    services::evidence_requests::EvidenceRequestService,
    validate,
    validation::Validation,
};

#[derive(Clone)]
pub struct EvidenceRequestState {
    pub service: EvidenceRequestService,
    pub route_auth: EvidenceRequestRouteAuthState,
}

#[derive(Clone)]
pub struct EvidenceRequestRouteAuthState {
    pub authenticator: ApiKeyAuthenticator,
    pub authorizer: EvidenceRequestAuthorizer,
}

pub fn router(state: EvidenceRequestState) -> Router {
    let route_auth = state.route_auth.clone();

    Router::new()
        .route(
            "/workspaces/{workspace_id}/evidence-requests",
            post(create_evidence_request).get(list_evidence_requests),
        )
        .route(
            "/workspaces/{workspace_id}/evidence-requests/due",
            get(list_due_evidence_requests),
        )
        .route(
            "/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}",
            get(get_evidence_request).put(replace_evidence_request),
        )
        .route_layer(middleware::from_fn_with_state(
            route_auth,
            authorize_evidence_request_route,
        ))
        .with_state(state)
}

async fn authorize_evidence_request_route(
    State(state): State<EvidenceRequestRouteAuthState>,
    Path(path): Path<HashMap<String, String>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let method = request.method().clone();
    let authorizer = state.authorizer.clone();

    let actor = authorize_workspace_route(&state.authenticator, &path, &mut request).await?;
    let workspace_id = actor.workspace_id;

    let allowed = match method {
        Method::GET => authorizer
            .can_read_evidence_requests(&actor)
            .await
            .map_err(|e| {
                error!(
                    method = %method,
                    actor = %actor.id,
                    workspace = %workspace_id,
                    error = %e,
                    "unable to check read permissions for evidence requests"
                );
                ApiError::Internal
            }),
        Method::POST | Method::PUT => authorizer
            .can_write_evidence_requests(&actor)
            .await
            .map_err(|e| {
                error!(
                    method = %method,
                    actor = %actor.id,
                    workspace = %workspace_id,
                    error = %e,
                    "unable to check write permissions for evidence requests"
                );
                ApiError::Internal
            }),
        _ => Err(ApiError::MethodNotAllowed),
    }?;

    if !allowed {
        return Err(ApiError::NotFound);
    }

    Ok(next.run(request).await)
}

#[derive(Debug, Deserialize)]
struct EvidenceRequestDTO {
    title: String,
    description: String,
    collection_instructions: String,
    cadence: String,
    due_at: DateTime<Utc>,
    schedule_anchor_at: DateTime<Utc>,
    freshness_window_days: Option<i32>,
    status: String,
}

impl EvidenceRequestDTO {
    fn into_new(self) -> Validation<CreateEvidenceRequestPayload, DomainError> {
        validate! {
            title <- required_text("title", self.title),
            description <- required_text("description", self.description),
            collection_instructions <- required_text(
                "collection_instructions",
                self.collection_instructions
            ),
            cadence <- parse_cadence(self.cadence),
            freshness_window_days <- validate_freshness_window_days(self.freshness_window_days),
            status <- parse_status(self.status),
            => CreateEvidenceRequestPayload {
                title,
                description,
                collection_instructions,
                cadence,
                due_at: self.due_at,
                schedule_anchor_at: self.schedule_anchor_at,
                freshness_window_days,
                status,
            },
        }
    }

    fn into_update(self) -> Validation<UpdateEvidenceRequestPayload, DomainError> {
        validate! {
            title <- required_text("title", self.title),
            description <- required_text("description", self.description),
            collection_instructions <- required_text(
                "collection_instructions",
                self.collection_instructions
            ),
            cadence <- parse_cadence(self.cadence),
            freshness_window_days <- validate_freshness_window_days(self.freshness_window_days),
            status <- parse_status(self.status),
            => UpdateEvidenceRequestPayload {
                title,
                description,
                collection_instructions,
                cadence,
                due_at: self.due_at,
                schedule_anchor_at: self.schedule_anchor_at,
                freshness_window_days,
                status,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct DueQuery {
    now: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct EvidenceRequestResponse {
    id: Uuid,
    workspace_id: Uuid,
    title: String,
    description: String,
    collection_instructions: String,
    cadence: &'static str,
    due_at: DateTime<Utc>,
    schedule_anchor_at: DateTime<Utc>,
    freshness_window_days: Option<i32>,
    status: &'static str,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<EvidenceRequest> for EvidenceRequestResponse {
    fn from(request: EvidenceRequest) -> Self {
        Self {
            id: Uuid::from(request.id),
            workspace_id: Uuid::from(request.workspace_id),
            title: request.title,
            description: request.description,
            collection_instructions: request.collection_instructions,
            cadence: request.cadence.as_str(),
            due_at: request.due_at,
            schedule_anchor_at: request.schedule_anchor_at,
            freshness_window_days: request.freshness_window_days,
            status: request.status.as_str(),
            created_at: request.created_at,
            updated_at: request.updated_at,
        }
    }
}

async fn create_evidence_request(
    State(state): State<EvidenceRequestState>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<EvidenceRequestDTO>,
) -> Result<Json<EvidenceRequestResponse>, ApiError> {
    let workspace_id = WorkspaceId::from(workspace_id);
    let request = body.into_new().into_result().map_err(domain_errors)?;
    let request = state.service.create(workspace_id, request).await?;

    Ok(Json(request.into()))
}

async fn list_evidence_requests(
    State(state): State<EvidenceRequestState>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<EvidenceRequestResponse>>, ApiError> {
    let requests = state
        .service
        .list_by_workspace(WorkspaceId::from(workspace_id))
        .await?;

    Ok(Json(requests.into_iter().map(Into::into).collect()))
}

async fn list_due_evidence_requests(
    State(state): State<EvidenceRequestState>,
    Query(query): Query<DueQuery>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<EvidenceRequestResponse>>, ApiError> {
    let workspace_id = WorkspaceId::from(workspace_id);
    let requests = state
        .service
        .list_due(workspace_id, query.now.unwrap_or_else(Utc::now))
        .await?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(Json(requests))
}

async fn get_evidence_request(
    State(state): State<EvidenceRequestState>,
    Path((workspace_id, evidence_request_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<EvidenceRequestResponse>, ApiError> {
    let workspace_id = WorkspaceId::from(workspace_id);
    let request = state
        .service
        .get(workspace_id, EvidenceRequestId::from(evidence_request_id))
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(request.into()))
}

async fn replace_evidence_request(
    State(state): State<EvidenceRequestState>,
    Path((workspace_id, evidence_request_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<EvidenceRequestDTO>,
) -> Result<Json<EvidenceRequestResponse>, ApiError> {
    let workspace_id = WorkspaceId::from(workspace_id);
    let evidence_request_id = EvidenceRequestId::from(evidence_request_id);
    let update = body.into_update().into_result().map_err(domain_errors)?;
    let request = state
        .service
        .replace(workspace_id, evidence_request_id, update)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(request.into()))
}

fn parse_cadence(value: String) -> Validation<EvidenceRequestCadence, DomainError> {
    value
        .parse::<EvidenceRequestCadence>()
        .map(Validation::valid)
        .unwrap_or_else(Validation::invalid)
}

fn parse_status(value: String) -> Validation<EvidenceRequestStatus, DomainError> {
    value
        .parse::<EvidenceRequestStatus>()
        .map(Validation::valid)
        .unwrap_or_else(Validation::invalid)
}

#[cfg(test)]
mod tests {
    use super::EvidenceRequestDTO;
    use crate::domain::{DomainError, EvidenceRequestCadence, EvidenceRequestStatus};
    use chrono::{DateTime, Utc};

    #[test]
    fn request_dto_maps_to_create_payload() {
        let payload = valid_dto().into_new().into_result().unwrap();

        assert_eq!(payload.title, "Quarterly access review");
        assert_eq!(payload.cadence, EvidenceRequestCadence::Quarterly);
        assert_eq!(payload.status, EvidenceRequestStatus::Active);
    }

    #[test]
    fn request_dto_accumulates_validation_errors() {
        let errors = EvidenceRequestDTO {
            title: String::new(),
            description: " ".to_owned(),
            collection_instructions: "\t".to_owned(),
            cadence: "weekly".to_owned(),
            due_at: unix_epoch(),
            schedule_anchor_at: unix_epoch(),
            freshness_window_days: Some(0),
            status: "draft".to_owned(),
        }
        .into_new()
        .into_result()
        .unwrap_err();

        assert_eq!(
            errors,
            vec![
                DomainError::EmptyRequiredText { field: "title" },
                DomainError::EmptyRequiredText {
                    field: "description"
                },
                DomainError::EmptyRequiredText {
                    field: "collection_instructions"
                },
                DomainError::InvalidEnumValue {
                    field: "cadence",
                    value: "weekly".to_owned()
                },
                DomainError::InvalidFreshnessWindowDays,
                DomainError::InvalidEnumValue {
                    field: "status",
                    value: "draft".to_owned()
                },
            ]
        );
    }

    fn valid_dto() -> EvidenceRequestDTO {
        EvidenceRequestDTO {
            title: "Quarterly access review".to_owned(),
            description: "Confirm quarterly access reviews are completed.".to_owned(),
            collection_instructions: "Export the completed review report.".to_owned(),
            cadence: "quarterly".to_owned(),
            due_at: unix_epoch(),
            schedule_anchor_at: unix_epoch(),
            freshness_window_days: Some(90),
            status: "active".to_owned(),
        }
    }

    fn unix_epoch() -> DateTime<Utc> {
        DateTime::<Utc>::UNIX_EPOCH
    }
}
