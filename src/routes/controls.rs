use std::collections::HashMap;

use axum::{
    extract::{Path, Request, State},
    http::Method,
    middleware,
    middleware::Next,
    response::Response,
    routing::get,
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
        required_text, Control, ControlId, CreateControlPayload,
        CreateEvidenceRequestControlMappingPayload, DomainError, EvidenceRequestControlMapping,
        EvidenceRequestId, Framework, FrameworkId, FrameworkRequirement, FrameworkRequirementId,
        UpdateControlPayload, WorkspaceId,
    },
    routes::{
        authentication::authorize_workspace_route,
        error::{domain_errors, ApiError},
    },
    services::controls::ControlService,
    validate,
    validation::Validation,
};

#[derive(Clone)]
pub struct ControlState {
    pub service: ControlService,
    pub route_auth: ControlRouteAuthState,
}

#[derive(Clone)]
pub struct ControlRouteAuthState {
    pub authenticator: ApiKeyAuthenticator,
    pub authorizer: EvidenceRequestAuthorizer,
}

pub fn router(state: ControlState) -> Router {
    let route_auth = state.route_auth.clone();

    Router::new()
        .route("/workspaces/{workspace_id}/frameworks", get(list_frameworks))
        .route(
            "/workspaces/{workspace_id}/frameworks/{framework_id}/requirements",
            get(list_framework_requirements),
        )
        .route(
            "/workspaces/{workspace_id}/controls",
            get(list_controls).post(create_control),
        )
        .route(
            "/workspaces/{workspace_id}/controls/{control_id}",
            get(get_control).put(replace_control),
        )
        .route(
            "/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/control-mappings",
            get(list_evidence_request_control_mappings).post(create_evidence_request_control_mapping),
        )
        .route(
            "/workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/control-mappings/{control_id}",
            axum::routing::delete(delete_evidence_request_control_mapping),
        )
        .route_layer(middleware::from_fn_with_state(
            route_auth,
            authorize_control_route,
        ))
        .with_state(state)
}

async fn authorize_control_route(
    State(state): State<ControlRouteAuthState>,
    Path(path): Path<HashMap<String, String>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let method = request.method().clone();
    let authorizer = state.authorizer.clone();
    let actor = authorize_workspace_route(&state.authenticator, &path, &mut request).await?;
    let workspace_id = actor.workspace_id;

    let allowed = match method {
        Method::GET => authorizer.can_read_controls(&actor).await.map_err(|e| {
            error!(
                method = %method,
                actor = %actor.id,
                workspace = %workspace_id,
                error = %e,
                "unable to check read permissions for controls"
            );
            ApiError::Internal
        }),
        Method::POST | Method::PUT | Method::DELETE => {
            authorizer.can_write_controls(&actor).await.map_err(|e| {
                error!(
                    method = %method,
                    actor = %actor.id,
                    workspace = %workspace_id,
                    error = %e,
                    "unable to check write permissions for controls"
                );
                ApiError::Internal
            })
        }
        _ => Err(ApiError::MethodNotAllowed),
    }?;

    if !allowed {
        return Err(ApiError::NotFound);
    }

    Ok(next.run(request).await)
}

#[derive(Debug, Deserialize)]
struct ControlDTO {
    code: String,
    title: String,
    description: String,
    framework_requirement_ids: Vec<Uuid>,
}

impl ControlDTO {
    fn into_new(self) -> Validation<CreateControlPayload, DomainError> {
        validate! {
            code <- required_text("code", self.code),
            title <- required_text("title", self.title),
            description <- required_text("description", self.description),
            framework_requirement_ids <- parse_framework_requirement_ids(self.framework_requirement_ids),
            => CreateControlPayload {
                code,
                title,
                description,
                framework_requirement_ids,
            },
        }
    }

    fn into_update(self) -> Validation<UpdateControlPayload, DomainError> {
        validate! {
            code <- required_text("code", self.code),
            title <- required_text("title", self.title),
            description <- required_text("description", self.description),
            framework_requirement_ids <- parse_framework_requirement_ids(self.framework_requirement_ids),
            => UpdateControlPayload {
                code,
                title,
                description,
                framework_requirement_ids,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct MappingDTO {
    control_id: Uuid,
    rationale: String,
}

impl MappingDTO {
    fn into_new(
        self,
        evidence_request_id: EvidenceRequestId,
    ) -> Validation<CreateEvidenceRequestControlMappingPayload, DomainError> {
        validate! {
            rationale <- required_text("rationale", self.rationale),
            => CreateEvidenceRequestControlMappingPayload {
                evidence_request_id,
                control_id: ControlId::from(self.control_id),
                rationale,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct FrameworkResponse {
    id: Uuid,
    code: String,
    name: String,
    description: String,
}

impl From<Framework> for FrameworkResponse {
    fn from(framework: Framework) -> Self {
        Self {
            id: Uuid::from(framework.id),
            code: framework.code,
            name: framework.name,
            description: framework.description,
        }
    }
}

#[derive(Debug, Serialize)]
struct FrameworkRequirementResponse {
    id: Uuid,
    framework_id: Uuid,
    code: String,
    title: String,
    description: String,
}

impl From<FrameworkRequirement> for FrameworkRequirementResponse {
    fn from(requirement: FrameworkRequirement) -> Self {
        Self {
            id: Uuid::from(requirement.id),
            framework_id: Uuid::from(requirement.framework_id),
            code: requirement.code,
            title: requirement.title,
            description: requirement.description,
        }
    }
}

#[derive(Debug, Serialize)]
struct ControlResponse {
    id: Uuid,
    workspace_id: Uuid,
    code: String,
    title: String,
    description: String,
    framework_requirements: Vec<FrameworkRequirementResponse>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Control> for ControlResponse {
    fn from(control: Control) -> Self {
        Self {
            id: Uuid::from(control.id),
            workspace_id: Uuid::from(control.workspace_id),
            code: control.code,
            title: control.title,
            description: control.description,
            framework_requirements: control
                .framework_requirements
                .into_iter()
                .map(Into::into)
                .collect(),
            created_at: control.created_at,
            updated_at: control.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct ControlSummaryResponse {
    id: Uuid,
    code: String,
    title: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct EvidenceRequestControlMappingResponse {
    evidence_request_id: Uuid,
    control: ControlSummaryResponse,
    rationale: String,
    created_at: DateTime<Utc>,
}

impl From<EvidenceRequestControlMapping> for EvidenceRequestControlMappingResponse {
    fn from(mapping: EvidenceRequestControlMapping) -> Self {
        Self {
            evidence_request_id: Uuid::from(mapping.evidence_request_id),
            control: ControlSummaryResponse {
                id: Uuid::from(mapping.control.id),
                code: mapping.control.code,
                title: mapping.control.title,
                description: mapping.control.description,
            },
            rationale: mapping.rationale,
            created_at: mapping.created_at,
        }
    }
}

async fn list_frameworks(
    State(state): State<ControlState>,
    Path(_workspace_id): Path<Uuid>,
) -> Result<Json<Vec<FrameworkResponse>>, ApiError> {
    let frameworks = state.service.list_frameworks().await?;

    Ok(Json(frameworks.into_iter().map(Into::into).collect()))
}

async fn list_framework_requirements(
    State(state): State<ControlState>,
    Path((_workspace_id, framework_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<FrameworkRequirementResponse>>, ApiError> {
    let requirements = state
        .service
        .list_framework_requirements(FrameworkId::from(framework_id))
        .await?;
    if requirements.is_empty() {
        return Err(ApiError::NotFound);
    }

    Ok(Json(requirements.into_iter().map(Into::into).collect()))
}

async fn create_control(
    State(state): State<ControlState>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<ControlDTO>,
) -> Result<Json<ControlResponse>, ApiError> {
    let workspace_id = WorkspaceId::from(workspace_id);
    let payload = body.into_new().into_result().map_err(domain_errors)?;
    let control = state
        .service
        .create_control(workspace_id, payload)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(control.into()))
}

async fn list_controls(
    State(state): State<ControlState>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<ControlResponse>>, ApiError> {
    let controls = state
        .service
        .list_controls(WorkspaceId::from(workspace_id))
        .await?;

    Ok(Json(controls.into_iter().map(Into::into).collect()))
}

async fn get_control(
    State(state): State<ControlState>,
    Path((workspace_id, control_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ControlResponse>, ApiError> {
    let control = state
        .service
        .get_control(WorkspaceId::from(workspace_id), ControlId::from(control_id))
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(control.into()))
}

async fn replace_control(
    State(state): State<ControlState>,
    Path((workspace_id, control_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ControlDTO>,
) -> Result<Json<ControlResponse>, ApiError> {
    let payload = body.into_update().into_result().map_err(domain_errors)?;
    let control = state
        .service
        .replace_control(
            WorkspaceId::from(workspace_id),
            ControlId::from(control_id),
            payload,
        )
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(control.into()))
}

async fn create_evidence_request_control_mapping(
    State(state): State<ControlState>,
    Path((workspace_id, evidence_request_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MappingDTO>,
) -> Result<Json<EvidenceRequestControlMappingResponse>, ApiError> {
    let payload = body
        .into_new(EvidenceRequestId::from(evidence_request_id))
        .into_result()
        .map_err(domain_errors)?;
    let mapping = state
        .service
        .create_evidence_request_control_mapping(WorkspaceId::from(workspace_id), payload)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(mapping.into()))
}

async fn list_evidence_request_control_mappings(
    State(state): State<ControlState>,
    Path((workspace_id, evidence_request_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<EvidenceRequestControlMappingResponse>>, ApiError> {
    let mappings = state
        .service
        .list_evidence_request_control_mappings(
            WorkspaceId::from(workspace_id),
            EvidenceRequestId::from(evidence_request_id),
        )
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(mappings.into_iter().map(Into::into).collect()))
}

async fn delete_evidence_request_control_mapping(
    State(state): State<ControlState>,
    Path((workspace_id, evidence_request_id, control_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<axum::http::StatusCode, ApiError> {
    let deleted = state
        .service
        .delete_evidence_request_control_mapping(
            WorkspaceId::from(workspace_id),
            EvidenceRequestId::from(evidence_request_id),
            ControlId::from(control_id),
        )
        .await?;

    if !deleted {
        return Err(ApiError::NotFound);
    }

    Ok(axum::http::StatusCode::NO_CONTENT)
}

fn parse_framework_requirement_ids(
    ids: Vec<Uuid>,
) -> Validation<Vec<FrameworkRequirementId>, DomainError> {
    Validation::valid(ids.into_iter().map(FrameworkRequirementId::from).collect())
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{ControlDTO, MappingDTO};
    use crate::domain::{DomainError, EvidenceRequestId};

    #[test]
    fn control_dto_maps_to_create_payload() {
        let requirement_id = Uuid::new_v4();
        let payload = ControlDTO {
            code: "CC6.1-ACCESS".to_owned(),
            title: "Access reviews".to_owned(),
            description: "Review production access quarterly.".to_owned(),
            framework_requirement_ids: vec![requirement_id],
        }
        .into_new()
        .into_result()
        .unwrap();

        assert_eq!(payload.code, "CC6.1-ACCESS");
        assert_eq!(
            payload.framework_requirement_ids,
            vec![requirement_id.into()]
        );
    }

    #[test]
    fn control_dto_accumulates_validation_errors() {
        let errors = ControlDTO {
            code: String::new(),
            title: " ".to_owned(),
            description: "\t".to_owned(),
            framework_requirement_ids: Vec::new(),
        }
        .into_new()
        .into_result()
        .unwrap_err();

        assert_eq!(
            errors,
            vec![
                DomainError::EmptyRequiredText { field: "code" },
                DomainError::EmptyRequiredText { field: "title" },
                DomainError::EmptyRequiredText {
                    field: "description"
                },
            ]
        );
    }

    #[test]
    fn mapping_dto_requires_rationale() {
        let errors = MappingDTO {
            control_id: Uuid::new_v4(),
            rationale: " ".to_owned(),
        }
        .into_new(EvidenceRequestId::from(Uuid::new_v4()))
        .into_result()
        .unwrap_err();

        assert_eq!(
            errors,
            vec![DomainError::EmptyRequiredText { field: "rationale" }]
        );
    }
}
