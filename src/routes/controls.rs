use std::collections::HashMap;

use axum::{
    extract::{Path, Request, State},
    http::Method,
    middleware,
    middleware::Next,
    response::Response,
    routing::get,
    Extension, Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    authentication::ApiTokenAuthenticator,
    authentication::ApiTokenContext,
    domain::{
        required_text, Control, ControlId, CreateControlPayload,
        CreateEvidenceRequestControlMappingPayload, DomainError, EvidenceRequestControlMapping,
        EvidenceRequestId, Framework, FrameworkId, FrameworkRequirement, FrameworkRequirementId,
        UpdateControlPayload, WorkspacePermission,
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
    pub authenticator: ApiTokenAuthenticator,
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
    let token = authorize_workspace_route(&state.authenticator, &path, &mut request).await?;

    let required = match method {
        Method::GET => WorkspacePermission::ReadControls,
        Method::POST | Method::PUT | Method::DELETE => WorkspacePermission::WriteControls,
        _ => return Err(ApiError::MethodNotAllowed),
    };

    if !token.permissions.has(required) {
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

type CreateControlRequest = ControlDTO;
type ReplaceControlRequest = ControlDTO;

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
struct CreateEvidenceRequestControlMappingRequest {
    control_id: Uuid,
    rationale: String,
}

impl CreateEvidenceRequestControlMappingRequest {
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
struct FrameworkResponseDTO {
    id: Uuid,
    code: String,
    name: String,
    description: String,
}

type ListFrameworksResponse = Vec<FrameworkResponseDTO>;

impl From<Framework> for FrameworkResponseDTO {
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
struct FrameworkRequirementResponseDTO {
    id: Uuid,
    framework_id: Uuid,
    code: String,
    title: String,
    description: String,
}

type ListFrameworkRequirementsResponse = Vec<FrameworkRequirementResponseDTO>;

impl From<FrameworkRequirement> for FrameworkRequirementResponseDTO {
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
struct ControlResponseDTO {
    id: Uuid,
    workspace_id: Uuid,
    code: String,
    title: String,
    description: String,
    framework_requirements: Vec<FrameworkRequirementResponseDTO>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

type ControlResponse = ControlResponseDTO;
type ListControlsResponse = Vec<ControlResponseDTO>;

impl From<Control> for ControlResponseDTO {
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
struct ControlSummaryResponseDTO {
    id: Uuid,
    code: String,
    title: String,
    description: String,
}

#[derive(Debug, Serialize)]
struct EvidenceRequestControlMappingResponseDTO {
    evidence_request_id: Uuid,
    control: ControlSummaryResponseDTO,
    rationale: String,
    created_at: DateTime<Utc>,
}

type EvidenceRequestControlMappingResponse = EvidenceRequestControlMappingResponseDTO;
type ListEvidenceRequestControlMappingsResponse = Vec<EvidenceRequestControlMappingResponseDTO>;

impl From<EvidenceRequestControlMapping> for EvidenceRequestControlMappingResponseDTO {
    fn from(mapping: EvidenceRequestControlMapping) -> Self {
        Self {
            evidence_request_id: Uuid::from(mapping.evidence_request_id),
            control: ControlSummaryResponseDTO {
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

#[derive(Debug, Deserialize)]
struct FrameworkRequirementsPath {
    framework_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct ControlPath {
    control_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct EvidenceRequestControlMappingsPath {
    evidence_request_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct EvidenceRequestControlMappingPath {
    evidence_request_id: Uuid,
    control_id: Uuid,
}

async fn list_frameworks(
    State(state): State<ControlState>,
) -> Result<Json<ListFrameworksResponse>, ApiError> {
    let frameworks = state.service.list_frameworks().await?;

    Ok(Json(frameworks.into_iter().map(Into::into).collect()))
}

async fn list_framework_requirements(
    State(state): State<ControlState>,
    Path(path): Path<FrameworkRequirementsPath>,
) -> Result<Json<ListFrameworkRequirementsResponse>, ApiError> {
    let requirements = state
        .service
        .list_framework_requirements(FrameworkId::from(path.framework_id))
        .await?;
    if requirements.is_empty() {
        return Err(ApiError::NotFound);
    }

    Ok(Json(requirements.into_iter().map(Into::into).collect()))
}

async fn create_control(
    State(state): State<ControlState>,
    Extension(token): Extension<ApiTokenContext>,
    Json(body): Json<CreateControlRequest>,
) -> Result<Json<ControlResponse>, ApiError> {
    let payload = body.into_new().into_result().map_err(domain_errors)?;
    let control = state.service.create_control(token, payload).await?;

    Ok(Json(control.into()))
}

async fn list_controls(
    State(state): State<ControlState>,
    Extension(token): Extension<ApiTokenContext>,
) -> Result<Json<ListControlsResponse>, ApiError> {
    let controls = state.service.list_controls(token).await?;

    Ok(Json(controls.into_iter().map(Into::into).collect()))
}

async fn get_control(
    State(state): State<ControlState>,
    Path(path): Path<ControlPath>,
    Extension(token): Extension<ApiTokenContext>,
) -> Result<Json<ControlResponse>, ApiError> {
    let control = state
        .service
        .get_control(token, ControlId::from(path.control_id))
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(control.into()))
}

async fn replace_control(
    State(state): State<ControlState>,
    Path(path): Path<ControlPath>,
    Extension(token): Extension<ApiTokenContext>,
    Json(body): Json<ReplaceControlRequest>,
) -> Result<Json<ControlResponse>, ApiError> {
    let payload = body.into_update().into_result().map_err(domain_errors)?;
    let control = state
        .service
        .replace_control(token, ControlId::from(path.control_id), payload)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(control.into()))
}

async fn create_evidence_request_control_mapping(
    State(state): State<ControlState>,
    Path(path): Path<EvidenceRequestControlMappingsPath>,
    Extension(token): Extension<ApiTokenContext>,
    Json(body): Json<CreateEvidenceRequestControlMappingRequest>,
) -> Result<Json<EvidenceRequestControlMappingResponse>, ApiError> {
    let payload = body
        .into_new(EvidenceRequestId::from(path.evidence_request_id))
        .into_result()
        .map_err(domain_errors)?;
    let mapping = state
        .service
        .create_evidence_request_control_mapping(token, payload)
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(mapping.into()))
}

async fn list_evidence_request_control_mappings(
    State(state): State<ControlState>,
    Path(path): Path<EvidenceRequestControlMappingsPath>,
    Extension(token): Extension<ApiTokenContext>,
) -> Result<Json<ListEvidenceRequestControlMappingsResponse>, ApiError> {
    let mappings = state
        .service
        .list_evidence_request_control_mappings(
            token,
            EvidenceRequestId::from(path.evidence_request_id),
        )
        .await?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(mappings.into_iter().map(Into::into).collect()))
}

async fn delete_evidence_request_control_mapping(
    State(state): State<ControlState>,
    Path(path): Path<EvidenceRequestControlMappingPath>,
    Extension(token): Extension<ApiTokenContext>,
) -> Result<axum::http::StatusCode, ApiError> {
    let deleted = state
        .service
        .delete_evidence_request_control_mapping(
            token,
            EvidenceRequestId::from(path.evidence_request_id),
            ControlId::from(path.control_id),
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

    use super::{ControlDTO, CreateEvidenceRequestControlMappingRequest};
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
    fn mapping_request_requires_rationale() {
        let errors = CreateEvidenceRequestControlMappingRequest {
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
