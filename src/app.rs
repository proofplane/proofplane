use std::{sync::Arc, time::Duration};

use crate::{
    application::{
        commands::{
            agent_connections::RevokeAgentConnectionHandler,
            claim_auditor_auth_transaction::ClaimAuditorAuthTransactionHandler,
            complete_auditor_authentication::CompleteAuditorAuthenticationHandler,
            create_authenticated_auditor_session::CreateAuthenticatedAuditorSessionHandler,
            create_owned_workspace::CreateOwnedWorkspaceHandler,
            issue_agent_evidence_upload_grant::AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE,
            record_user_login::RecordUserLoginHandler,
            redeem_evidence_document_upload_grant::RedeemEvidenceDocumentUploadGrantHandler,
            redeem_policy_document_upload_grant::RedeemPolicyDocumentUploadGrantHandler,
            remove_workspace_member::RemoveWorkspaceMemberHandler,
            revoke_auditor_session::RevokeAuditorSessionHandler,
            start_auditor_auth_transaction::StartAuditorAuthTransactionHandler,
        },
        queries::{
            agent_connections::ListUserAgentConnectionsHandler, get_user::GetUserHandler,
            get_workspace_for_user::GetWorkspaceForUserHandler, policy_catalog::GetPolicyHandler,
            read_auditor_portal::ReadAuditorPortalHandler,
            resolve_active_auditor_grant::ResolveActiveAuditorGrantHandler,
            resolve_auditor_grant_by_secret::ResolveAuditorGrantBySecretHandler,
            resolve_auditor_session_by_digest::ResolveAuditorSessionByDigestHandler,
            resolve_evidence_document_upload_grant_authority::ResolveEvidenceDocumentUploadGrantAuthorityHandler,
            resolve_policy_document_upload_grant_authority::ResolvePolicyDocumentUploadGrantAuthorityHandler,
        },
    },
    authentication::{
        auth0::{
            Auth0AuditorIdentityProvider, SharedAuditorIdentityProvider, TokenVerifier,
            VerifiedClaims,
        },
        paseto::{
            AgentEvidenceUploadGrantDecryptor, AgentPolicyDocumentUploadGrantDecryptor,
            AgentPolicyDocumentUploadGrantEncryptor, DownloadGrantDecryptor,
            DownloadGrantEncryptor, McpOAuthDecryptor, McpOAuthEncryptor,
            PolicyUploadGrantDecryptor, PolicyUploadSessionDecryptor, PolicyUploadSessionEncryptor,
            UploadGrantDecryptor, UploadSessionDecryptor, UploadSessionEncryptor,
        },
        Error as AuthenticationError, UserAuthenticator,
    },
    config::AppConfig,
    object_storage::FilesystemObjectStore,
    repository::Postgres,
    routes::{
        agent_connections::{self, AgentConnectionsState},
        agent_evidence_uploads::{self, AgentEvidenceUploadState},
        agent_policy_document_uploads::{self, AgentPolicyDocumentUploadState},
        auditor_access::{self, AuditorAccessState},
        document_downloads::{self, DocumentDownloadState},
        document_upload_sessions::{self, DocumentUploadSessionState},
        error::not_found,
        health::{self, ReadyState},
        me::{self, MeState, UserRouteAuthState},
        metrics::{self, MetricsState},
        oauth::{self, OAuthState},
        policy_document_upload_sessions::{self, PolicyDocumentUploadSessionState},
        request_context::attach_request_id,
        version,
        workspaces::{self, WorkspacesState},
    },
    services::{
        agent_evidence_upload_grants::AgentEvidenceUploadCredentialVerifier,
        agent_evidence_uploads::AgentEvidenceUploadService,
        agent_policy_document_upload_grants::{
            AgentPolicyDocumentUploadGrantService, AGENT_POLICY_DOCUMENT_UPLOAD_GRANT_AUDIENCE,
        },
        agent_policy_document_uploads::AgentPolicyDocumentUploadService,
        client_resolver::ClientResolver,
        controls::ControlService,
        document_downloads::DocumentDownloadService,
        evidence_submissions::EvidenceSubmissionService,
        oauth::OAuthService,
        policy_document_upload_grants::POLICY_UPLOAD_GRANT_AUDIENCE,
        policy_documents::PolicyDocumentService,
        policy_upload_sessions::{PolicyUploadSessionTokenService, POLICY_UPLOAD_SESSION_AUDIENCE},
        upload_sessions::{UploadSessionTokenService, UPLOAD_SESSION_AUDIENCE},
    },
};
use axum::{extract::MatchedPath, http::Request, middleware, response::Response, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::Span;

pub struct AppDependencies<V: TokenVerifier<Claims = VerifiedClaims>> {
    pub config: AppConfig,
    pub postgres: Arc<Postgres>,
    pub object_store: Arc<FilesystemObjectStore>,
    pub metrics: PrometheusHandle,
    pub user_authenticator: UserAuthenticator<V>,
    pub auditor_identity_provider: Option<SharedAuditorIdentityProvider>,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateAppError {
    #[error("authentication initialization error")]
    Authentication(#[from] AuthenticationError),
}

pub fn create_app<V: TokenVerifier<Claims = VerifiedClaims> + 'static>(
    dependencies: AppDependencies<V>,
) -> Result<Router, CreateAppError> {
    let evidence_submission_service = EvidenceSubmissionService::new(
        dependencies.postgres.clone(),
        dependencies.object_store.clone(),
    );
    let agent_upload_grant_decryptor = AgentEvidenceUploadGrantDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE,
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(AuthenticationError::from)?;
    let agent_evidence_upload_service = AgentEvidenceUploadService::new(
        dependencies.postgres.clone(),
        evidence_submission_service.clone(),
        AgentEvidenceUploadCredentialVerifier::new(agent_upload_grant_decryptor),
        dependencies.config.uploads.max_document_bytes,
    );
    let agent_policy_upload_grant_encryptor = AgentPolicyDocumentUploadGrantEncryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        AGENT_POLICY_DOCUMENT_UPLOAD_GRANT_AUDIENCE,
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(AuthenticationError::from)?;
    let agent_policy_upload_grant_decryptor = AgentPolicyDocumentUploadGrantDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        AGENT_POLICY_DOCUMENT_UPLOAD_GRANT_AUDIENCE,
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(AuthenticationError::from)?;
    let agent_policy_upload_grant_service = AgentPolicyDocumentUploadGrantService::new(
        dependencies.postgres.clone(),
        agent_policy_upload_grant_encryptor,
        agent_policy_upload_grant_decryptor,
    );
    let agent_policy_document_upload_service = AgentPolicyDocumentUploadService::new(
        dependencies.postgres.clone(),
        dependencies.object_store.clone(),
        agent_policy_upload_grant_service.credential_verifier(),
        dependencies.config.uploads.max_document_bytes,
    );
    let download_grant_encryptor = DownloadGrantEncryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        "proofplane-document-download",
        &dependencies.config.paseto.download,
    )
    .map_err(AuthenticationError::from)?;
    let download_grant_decryptor = DownloadGrantDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        "proofplane-document-download",
        &dependencies.config.paseto.download,
    )
    .map_err(AuthenticationError::from)?;
    let document_download_service = DocumentDownloadService::new(
        dependencies.postgres.clone(),
        dependencies.object_store.clone(),
        dependencies.config.server.public_api_base_url.clone(),
        download_grant_encryptor,
        download_grant_decryptor,
    );
    let upload_grant_decryptor = UploadGrantDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        "proofplane-document-upload-grant",
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(AuthenticationError::from)?;
    let upload_session_encryptor = UploadSessionEncryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        UPLOAD_SESSION_AUDIENCE,
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(AuthenticationError::from)?;
    let upload_session_decryptor = UploadSessionDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        UPLOAD_SESSION_AUDIENCE,
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(AuthenticationError::from)?;
    let resolve_evidence_document_upload_grant =
        ResolveEvidenceDocumentUploadGrantAuthorityHandler::new(upload_grant_decryptor);
    let redeem_evidence_document_upload_grant =
        RedeemEvidenceDocumentUploadGrantHandler::new(dependencies.postgres.clone());
    let upload_session_service =
        UploadSessionTokenService::new(upload_session_encryptor, upload_session_decryptor);
    let policy_upload_grant_decryptor = PolicyUploadGrantDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        POLICY_UPLOAD_GRANT_AUDIENCE,
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(AuthenticationError::from)?;
    let policy_upload_session_encryptor = PolicyUploadSessionEncryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        POLICY_UPLOAD_SESSION_AUDIENCE,
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(AuthenticationError::from)?;
    let policy_upload_session_decryptor = PolicyUploadSessionDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        POLICY_UPLOAD_SESSION_AUDIENCE,
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(AuthenticationError::from)?;
    let resolve_policy_document_upload_grant =
        ResolvePolicyDocumentUploadGrantAuthorityHandler::new(policy_upload_grant_decryptor);
    let redeem_policy_document_upload_grant =
        RedeemPolicyDocumentUploadGrantHandler::new(dependencies.postgres.clone());
    let policy_upload_session_service = PolicyUploadSessionTokenService::new(
        policy_upload_session_encryptor,
        policy_upload_session_decryptor,
    );
    let policy_document_service = PolicyDocumentService::new(
        dependencies.postgres.clone(),
        dependencies.object_store.clone(),
    );
    let control_service = ControlService::new(dependencies.postgres.clone());
    let mcp_oauth_encryptor = McpOAuthEncryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        dependencies.config.mcp.resource.to_string(),
        &dependencies.config.paseto.mcp_oauth,
    )
    .map_err(AuthenticationError::from)?;
    let mcp_oauth_decryptor = McpOAuthDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        dependencies.config.mcp.resource.to_string(),
        &dependencies.config.paseto.mcp_oauth,
    )
    .map_err(AuthenticationError::from)?;
    let client_resolver =
        ClientResolver::from_mcp_oauth_config(&dependencies.config.paseto.mcp_oauth)
            .map_err(AuthenticationError::from)?;
    let oauth_service = OAuthService::new(
        dependencies.postgres.clone(),
        dependencies.config.server.public_api_base_url.clone(),
        dependencies.config.mcp.resource.clone(),
        client_resolver,
        mcp_oauth_encryptor,
        mcp_oauth_decryptor,
    );
    let secure_upload_cookie = dependencies.config.server.public_api_base_url.scheme() == "https";
    let secure_auditor_cookie = dependencies.config.server.public_api_base_url.scheme() == "https";
    let auditor_start_auth = StartAuditorAuthTransactionHandler::new(
        dependencies.postgres.clone(),
        dependencies.config.auth0.auditor_portal.clone(),
    );
    let auditor_claim_auth = ClaimAuditorAuthTransactionHandler::new(dependencies.postgres.clone());
    let auditor_create_session =
        CreateAuthenticatedAuditorSessionHandler::new(dependencies.postgres.clone());
    let auditor_identity_provider = dependencies.auditor_identity_provider.unwrap_or_else(|| {
        Arc::new(Auth0AuditorIdentityProvider::new(
            &dependencies.config.auth0,
        ))
    });
    let auditor_complete_auth = CompleteAuditorAuthenticationHandler::new(
        auditor_claim_auth.clone(),
        ResolveActiveAuditorGrantHandler::new(dependencies.postgres.clone()),
        auditor_create_session,
        auditor_identity_provider,
        dependencies.config.auth0.auditor_portal.clone(),
    );

    Ok(Router::new()
        .nest(
            &dependencies.config.health.live_path,
            health::livez_router(),
        )
        .nest(
            &dependencies.config.health.ready_path,
            health::readyz_router(ReadyState {
                postgres: dependencies.postgres.clone(),
                dependency_timeout_ms: dependencies.config.health.dependency_timeout_ms,
            }),
        )
        .nest(
            "/metrics",
            metrics::router(MetricsState {
                handle: dependencies.metrics,
            }),
        )
        .merge(document_downloads::router(DocumentDownloadState {
            service: document_download_service.clone(),
        }))
        .merge(agent_evidence_uploads::router(AgentEvidenceUploadState {
            service: agent_evidence_upload_service,
            max_document_bytes: dependencies.config.uploads.max_document_bytes,
        }))
        .merge(agent_policy_document_uploads::router(
            AgentPolicyDocumentUploadState {
                service: agent_policy_document_upload_service,
                max_document_bytes: dependencies.config.uploads.max_document_bytes,
            },
        ))
        .merge(document_upload_sessions::router(
            DocumentUploadSessionState {
                resolve_grant: resolve_evidence_document_upload_grant,
                redeem_grant: redeem_evidence_document_upload_grant,
                downloads: document_download_service.clone(),
                sessions: upload_session_service,
                submissions: evidence_submission_service,
                controls: control_service,
                secure_cookie: secure_upload_cookie,
                max_document_bytes: dependencies.config.uploads.max_document_bytes,
            },
        ))
        .merge(policy_document_upload_sessions::router(
            PolicyDocumentUploadSessionState {
                resolve_grant: resolve_policy_document_upload_grant,
                redeem_grant: redeem_policy_document_upload_grant,
                downloads: document_download_service.clone(),
                sessions: policy_upload_session_service,
                get_policy: GetPolicyHandler::new(dependencies.postgres.clone()),
                documents: policy_document_service,
                secure_cookie: secure_upload_cookie,
                max_document_bytes: dependencies.config.uploads.max_document_bytes,
            },
        ))
        .merge(auditor_access::router(AuditorAccessState {
            resolve_grant: ResolveAuditorGrantBySecretHandler::new(dependencies.postgres.clone()),
            start_auth: auditor_start_auth,
            claim_auth: auditor_claim_auth,
            complete_auth: auditor_complete_auth,
            resolve_session: ResolveAuditorSessionByDigestHandler::new(
                dependencies.postgres.clone(),
            ),
            revoke_session: RevokeAuditorSessionHandler::new(dependencies.postgres.clone()),
            read_portal: ReadAuditorPortalHandler::new(dependencies.postgres.clone()),
            downloads: document_download_service,
            secure_cookie: secure_auditor_cookie,
        }))
        .merge(me::router(MeState {
            get_user: GetUserHandler::new(dependencies.postgres.clone()),
            record_login: RecordUserLoginHandler::new(dependencies.postgres.clone()),
            route_auth: UserRouteAuthState {
                authenticator: dependencies.user_authenticator.clone(),
            },
        }))
        .merge(workspaces::router(WorkspacesState {
            create_owned: CreateOwnedWorkspaceHandler::new(dependencies.postgres.clone()),
            get_for_user: GetWorkspaceForUserHandler::new(dependencies.postgres.clone()),
            remove_member: RemoveWorkspaceMemberHandler::new(dependencies.postgres.clone()),
            route_auth: UserRouteAuthState {
                authenticator: dependencies.user_authenticator.clone(),
            },
        }))
        .merge(agent_connections::router(AgentConnectionsState {
            list: ListUserAgentConnectionsHandler::new(dependencies.postgres.clone()),
            revoke: RevokeAgentConnectionHandler::new(dependencies.postgres.clone()),
            route_auth: UserRouteAuthState {
                authenticator: dependencies.user_authenticator.clone(),
            },
            mcp_url: dependencies.config.mcp.resource.clone(),
        }))
        .merge(oauth::router(OAuthState {
            service: oauth_service,
            user_authenticator: dependencies.user_authenticator.clone(),
            auth0: dependencies.config.auth0.clone(),
            issuer: dependencies.config.server.public_api_base_url.clone(),
            resource: dependencies.config.mcp.resource.clone(),
            http: reqwest::Client::new(),
        }))
        .nest("/version", version::router())
        .fallback(not_found)
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

    use super::trace_path;

    #[test]
    fn http_trace_path_never_contains_query_parameters() {
        let request = Request::builder()
            .uri("/document-downloads?token=secret-jwt")
            .body(Body::empty())
            .unwrap();

        assert_eq!(trace_path(&request), "/document-downloads");
    }
}
