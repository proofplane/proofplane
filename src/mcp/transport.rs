use std::{sync::Arc, time::Duration};

use axum::{
    extract::MatchedPath,
    http::{header::InvalidHeaderValue, HeaderValue, Request},
    middleware,
    response::Response,
    Router,
};
use metrics_exporter_prometheus::PrometheusHandle;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::Span;

use super::{
    auth::{authenticate_request, AuthenticationState},
    server::{ProofplaneMcp, UploadDependencies},
};
use crate::authentication::paseto::{
    AgentEvidenceUploadGrantEncryptor, AgentPolicyDocumentUploadGrantEncryptor,
    DownloadGrantDecryptor, DownloadGrantEncryptor, PolicyUploadGrantDecryptor,
    PolicyUploadGrantEncryptor, UploadGrantDecryptor, UploadGrantEncryptor,
};
use crate::{
    application::{
        commands::{
            agent_connections::AuthorizeAgentConnectionHandler,
            create_control::CreateControlHandler,
            create_evidence::CreateEvidenceHandler,
            issue_agent_evidence_upload_grant::IssueAgentEvidenceUploadGrantHandler,
            issue_agent_policy_document_upload_grant::IssueAgentPolicyDocumentUploadGrantHandler,
            issue_auditor_access_grant::IssueAuditorAccessGrantHandler,
            issue_evidence_document_upload_grant::IssueEvidenceDocumentUploadGrantHandler,
            issue_policy_document_upload_grant::IssuePolicyDocumentUploadGrantHandler,
            map_control_to_evidence::MapControlToEvidenceHandler,
            map_evidence_to_controls::MapEvidenceToControlsHandler,
            policies::{
                ArchivePolicyHandler, AttachControlToPoliciesHandler,
                AttachPolicyToControlsHandler, CreatePolicyHandler,
                DetachControlFromPoliciesHandler, DetachPolicyFromControlsHandler,
                ReplacePolicyHandler,
            },
            replace_control::ReplaceControlHandler,
            revoke_auditor_access_grant::RevokeAuditorAccessGrantHandler,
            unmap_control_from_evidence::UnmapControlFromEvidenceHandler,
            unmap_evidence_from_controls::UnmapEvidenceFromControlsHandler,
        },
        queries::{
            control_catalog::{GetControlHandler, ListControlsHandler},
            evidence_catalog::{
                GetEvidenceHandler, ListEvidenceControlMappingsHandler, ListEvidenceHandler,
            },
            framework_catalog::{ListFrameworkRequirementsHandler, ListFrameworksHandler},
            list_auditor_access_grants::ListAuditorAccessGrantsHandler,
            policy_catalog::{GetPolicyHandler, ListPoliciesHandler},
        },
    },
    authentication::auth0::{TokenVerifier, VerifiedMcpClaims},
    config::HealthConfig,
    domain::WorkspacePermission,
    object_storage::FilesystemObjectStore,
    repository::Postgres,
    routes::{
        health::{self, ReadyState},
        metrics::{self, MetricsState},
        protected_resource_metadata::{
            self, ProtectedResourceMetadataState, PROTECTED_RESOURCE_METADATA_ENDPOINT,
        },
        request_context::attach_request_id,
    },
    services::evidence_submissions::EvidenceSubmissionService,
};
use url::Url;

pub const ENDPOINT: &str = "/mcp";

#[derive(Debug, thiserror::Error)]
pub enum McpAppError {
    #[error("MCP resource metadata URL cannot be constructed")]
    ResourceMetadataUrl(#[source] url::ParseError),
    #[error("MCP authentication challenge is not a valid HTTP header")]
    AuthenticationChallenge(#[source] InvalidHeaderValue),
}

pub struct McpAppDependencies<V> {
    pub postgres: Arc<Postgres>,
    pub object_store: Arc<FilesystemObjectStore>,
    pub metrics: PrometheusHandle,
    pub oauth_verifier: Arc<V>,
    pub authorization_server: Url,
    pub resource: Url,
    pub public_api_base_url: Url,
    pub download_grant_encryptor: DownloadGrantEncryptor,
    pub download_grant_decryptor: DownloadGrantDecryptor,
    pub upload_grant_encryptor: UploadGrantEncryptor,
    pub upload_grant_decryptor: UploadGrantDecryptor,
    pub agent_upload_grant_encryptor: AgentEvidenceUploadGrantEncryptor,
    pub agent_policy_upload_grant_encryptor: AgentPolicyDocumentUploadGrantEncryptor,
    pub policy_upload_grant_encryptor: PolicyUploadGrantEncryptor,
    pub policy_upload_grant_decryptor: PolicyUploadGrantDecryptor,
    pub max_document_bytes: u64,
    pub health: HealthConfig,
    pub allowed_hosts: Vec<String>,
    pub cancellation_token: CancellationToken,
}

impl<V> Clone for McpAppDependencies<V> {
    fn clone(&self) -> Self {
        Self {
            postgres: self.postgres.clone(),
            object_store: self.object_store.clone(),
            metrics: self.metrics.clone(),
            oauth_verifier: self.oauth_verifier.clone(),
            authorization_server: self.authorization_server.clone(),
            resource: self.resource.clone(),
            public_api_base_url: self.public_api_base_url.clone(),
            download_grant_encryptor: self.download_grant_encryptor.clone(),
            download_grant_decryptor: self.download_grant_decryptor.clone(),
            upload_grant_encryptor: self.upload_grant_encryptor.clone(),
            upload_grant_decryptor: self.upload_grant_decryptor.clone(),
            agent_upload_grant_encryptor: self.agent_upload_grant_encryptor.clone(),
            agent_policy_upload_grant_encryptor: self.agent_policy_upload_grant_encryptor.clone(),
            policy_upload_grant_encryptor: self.policy_upload_grant_encryptor.clone(),
            policy_upload_grant_decryptor: self.policy_upload_grant_decryptor.clone(),
            max_document_bytes: self.max_document_bytes,
            health: self.health.clone(),
            allowed_hosts: self.allowed_hosts.clone(),
            cancellation_token: self.cancellation_token.clone(),
        }
    }
}

pub fn create_app<V>(dependencies: McpAppDependencies<V>) -> Result<Router, McpAppError>
where
    V: TokenVerifier<Claims = VerifiedMcpClaims> + 'static,
{
    let evidence_submissions = EvidenceSubmissionService::new(
        dependencies.postgres.clone(),
        dependencies.object_store.clone(),
    );
    let issue_evidence_document_upload_grant = IssueEvidenceDocumentUploadGrantHandler::new(
        dependencies.postgres.clone(),
        dependencies.public_api_base_url.clone(),
        dependencies.upload_grant_encryptor,
    );
    let issue_agent_evidence_upload_grant = IssueAgentEvidenceUploadGrantHandler::new(
        dependencies.postgres.clone(),
        dependencies.agent_upload_grant_encryptor,
    );
    let issue_agent_policy_document_upload_grant = IssueAgentPolicyDocumentUploadGrantHandler::new(
        dependencies.postgres.clone(),
        dependencies.agent_policy_upload_grant_encryptor,
    );
    let issue_policy_document_upload_grant = IssuePolicyDocumentUploadGrantHandler::new(
        dependencies.postgres.clone(),
        dependencies.public_api_base_url.clone(),
        dependencies.policy_upload_grant_encryptor,
    );
    let authorize_agent_connection =
        AuthorizeAgentConnectionHandler::new(dependencies.postgres.clone());
    let protocol = protocol_router(
        dependencies.oauth_verifier,
        dependencies.resource.clone(),
        authorize_agent_connection,
        ProofplaneMcp::new(
            super::server::EvidenceHandlers {
                create: CreateEvidenceHandler::new(dependencies.postgres.clone()),
                list: ListEvidenceHandler::new(dependencies.postgres.clone()),
                get: GetEvidenceHandler::new(dependencies.postgres.clone()),
                list_control_mappings: ListEvidenceControlMappingsHandler::new(
                    dependencies.postgres.clone(),
                ),
                map_to_controls: MapEvidenceToControlsHandler::new(dependencies.postgres.clone()),
                map_control_to_evidence: MapControlToEvidenceHandler::new(
                    dependencies.postgres.clone(),
                ),
                unmap_from_controls: UnmapEvidenceFromControlsHandler::new(
                    dependencies.postgres.clone(),
                ),
                unmap_control_from_evidence: UnmapControlFromEvidenceHandler::new(
                    dependencies.postgres.clone(),
                ),
            },
            evidence_submissions,
            UploadDependencies {
                issue_evidence_grant: issue_evidence_document_upload_grant,
                issue_policy_grant: issue_policy_document_upload_grant,
                issue_agent_evidence_grant: issue_agent_evidence_upload_grant,
                issue_agent_policy_grant: issue_agent_policy_document_upload_grant,
                max_document_bytes: dependencies.max_document_bytes,
            },
            super::server::AuditorGrantHandlers {
                issue: IssueAuditorAccessGrantHandler::new(dependencies.postgres.clone()),
                list: ListAuditorAccessGrantsHandler::new(dependencies.postgres.clone()),
                revoke: RevokeAuditorAccessGrantHandler::new(dependencies.postgres.clone()),
            },
            super::server::ControlDependencies {
                handlers: super::server::ControlHandlers {
                    create: CreateControlHandler::new(dependencies.postgres.clone()),
                    replace: ReplaceControlHandler::new(dependencies.postgres.clone()),
                    list: ListControlsHandler::new(dependencies.postgres.clone()),
                    get: GetControlHandler::new(dependencies.postgres.clone()),
                    list_frameworks: ListFrameworksHandler::new(dependencies.postgres.clone()),
                    list_framework_requirements: ListFrameworkRequirementsHandler::new(
                        dependencies.postgres.clone(),
                    ),
                },
            },
            super::server::PolicyHandlers {
                create: CreatePolicyHandler::new(dependencies.postgres.clone()),
                replace: ReplacePolicyHandler::new(dependencies.postgres.clone()),
                archive: ArchivePolicyHandler::new(dependencies.postgres.clone()),
                attach_to_controls: AttachPolicyToControlsHandler::new(
                    dependencies.postgres.clone(),
                ),
                attach_control_to_policies: AttachControlToPoliciesHandler::new(
                    dependencies.postgres.clone(),
                ),
                detach_from_controls: DetachPolicyFromControlsHandler::new(
                    dependencies.postgres.clone(),
                ),
                detach_control_from_policies: DetachControlFromPoliciesHandler::new(
                    dependencies.postgres.clone(),
                ),
                list: ListPoliciesHandler::new(dependencies.postgres.clone()),
                get: GetPolicyHandler::new(dependencies.postgres.clone()),
            },
            dependencies.public_api_base_url,
        ),
        dependencies.allowed_hosts,
        dependencies.cancellation_token.clone(),
    )?;
    let protected_resource_metadata =
        protected_resource_metadata::router(ProtectedResourceMetadataState {
            resource: dependencies.resource.clone(),
            authorization_server: dependencies.authorization_server.clone(),
        });

    Ok(Router::new()
        .merge(protocol)
        .merge(protected_resource_metadata)
        .nest(&dependencies.health.live_path, health::livez_router())
        .nest(
            &dependencies.health.ready_path,
            health::readyz_router(ReadyState {
                postgres: dependencies.postgres,
                dependency_timeout_ms: dependencies.health.dependency_timeout_ms,
            }),
        )
        .nest(
            "/metrics",
            metrics::router(MetricsState {
                handle: dependencies.metrics,
            }),
        )
        .layer(middleware::from_fn(attach_request_id))
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    let method = request.method();
                    let path = trace_path(request);

                    tracing::info_span!(
                        "http_request",
                        %method,
                        path,
                        request_id = tracing::field::Empty,
                        user_id = tracing::field::Empty
                    )
                })
                .on_response(|response: &Response, latency: Duration, span: &Span| {
                    tracing::info!(
                        parent: span,
                        status = response.status().as_u16(),
                        latency_ms = latency.as_secs_f64() * 1000.0,
                        "http request completed"
                    );
                }),
        ))
}

pub fn protocol_router<V>(
    oauth_verifier: Arc<V>,
    resource: Url,
    authorize_agent_connection: AuthorizeAgentConnectionHandler,
    server: ProofplaneMcp,
    allowed_hosts: Vec<String>,
    cancellation_token: CancellationToken,
) -> Result<Router, McpAppError>
where
    V: TokenVerifier<Claims = VerifiedMcpClaims> + 'static,
{
    let challenge = authentication_challenge(&resource)?;
    let server_factory = move || Ok(server.clone());
    let mut transport_config = StreamableHttpServerConfig::default()
        .with_cancellation_token(cancellation_token.child_token());
    if !allowed_hosts.is_empty() {
        transport_config = transport_config.with_allowed_hosts(allowed_hosts);
    }
    let transport = StreamableHttpService::<ProofplaneMcp, LocalSessionManager>::new(
        server_factory,
        Default::default(),
        transport_config,
    );

    Ok(Router::new()
        .nest_service(ENDPOINT, transport)
        .layer(middleware::from_fn_with_state(
            AuthenticationState {
                auth0: oauth_verifier,
                authorize_agent_connection,
                resource: resource.to_string(),
                challenge,
            },
            authenticate_request,
        )))
}

fn authentication_challenge(resource: &Url) -> Result<HeaderValue, McpAppError> {
    let scopes = WorkspacePermission::ALL
        .iter()
        .map(|permission| permission.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let metadata = resource
        .join(PROTECTED_RESOURCE_METADATA_ENDPOINT)
        .map_err(McpAppError::ResourceMetadataUrl)?;
    let challenge = format!(
        "Bearer realm=\"proofplane-mcp\", resource_metadata=\"{metadata}\", scope=\"{scopes}\""
    );
    HeaderValue::from_str(&challenge).map_err(McpAppError::AuthenticationChallenge)
}

fn trace_path<B>(request: &Request<B>) -> &str {
    request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or_else(|| request.uri().path())
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use url::Url;

    use super::{authentication_challenge, trace_path, McpAppError};

    #[test]
    fn http_trace_path_never_contains_query_parameters() {
        let request = Request::builder()
            .uri("/mcp?session_id=secret")
            .body(Body::empty())
            .unwrap();

        assert_eq!(trace_path(&request), "/mcp");
    }

    #[test]
    fn authentication_challenge_rejects_non_hierarchical_resource_url() {
        let resource = Url::parse("mailto:mcp@proofplane.com").unwrap();

        assert!(matches!(
            authentication_challenge(&resource),
            Err(McpAppError::ResourceMetadataUrl(_))
        ));
    }
}
