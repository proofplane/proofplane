use std::{sync::Arc, time::Duration};

use crate::{
    authentication::{
        auth0::{TokenVerifier, VerifiedClaims},
        paseto::{
            DownloadGrantDecryptor, DownloadGrantEncryptor, McpOAuthDecryptor, McpOAuthEncryptor,
            UploadGrantDecryptor, UploadGrantEncryptor, UploadSessionDecryptor,
            UploadSessionEncryptor,
        },
        UserAuthenticator,
    },
    config::AppConfig,
    mailer::SharedMailAdapter,
    object_storage::FilesystemObjectStore,
    repository::Postgres,
    routes::{
        agent_connections::{self, AgentConnectionsState},
        auditor_access::{self, AuditorAccessState},
        document_downloads::{self, DocumentDownloadState},
        document_upload_sessions::{self, DocumentUploadSessionState},
        error::not_found,
        health::{self, ReadyState},
        me::{self, MeState, UserRouteAuthState},
        metrics::{self, MetricsState},
        oauth::{self, OAuthState},
        request_context::attach_request_id,
        version,
        workspaces::{self, WorkspacesState},
    },
    services::{
        agent_connections::AgentConnectionService,
        auditor_access_grants::AuditorAccessGrantService,
        auditor_access_sessions::AuditorAccessSessionService,
        auditor_portal::AuditorPortalReadModelService,
        client_resolver::ClientResolver,
        controls::ControlService,
        document_downloads::DocumentDownloadService,
        document_upload_grants::DocumentUploadGrantService,
        evidence_submissions::EvidenceSubmissionService,
        oauth::OAuthService,
        upload_sessions::{UploadSessionTokenService, UPLOAD_SESSION_AUDIENCE},
        user::UserService,
        workspaces::WorkspaceService,
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
    pub mail_adapter: Option<SharedMailAdapter>,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateAppError {
    #[error("authentication initialization error")]
    Authentication(#[from] crate::authentication::Error),
}

pub fn create_app<V: TokenVerifier<Claims = VerifiedClaims> + 'static>(
    dependencies: AppDependencies<V>,
) -> Result<Router, CreateAppError> {
    let evidence_submission_service = EvidenceSubmissionService::new(
        dependencies.postgres.clone(),
        dependencies.object_store.clone(),
    );
    let download_grant_encryptor = DownloadGrantEncryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        "proofplane-document-download",
        &dependencies.config.paseto.download,
    )
    .map_err(crate::authentication::Error::from)?;
    let download_grant_decryptor = DownloadGrantDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        "proofplane-document-download",
        &dependencies.config.paseto.download,
    )
    .map_err(crate::authentication::Error::from)?;
    let document_download_service = DocumentDownloadService::new(
        dependencies.postgres.clone(),
        dependencies.object_store.clone(),
        dependencies.config.server.public_api_base_url.clone(),
        download_grant_encryptor,
        download_grant_decryptor,
    );
    let upload_grant_encryptor = UploadGrantEncryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        "proofplane-document-upload-grant",
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(crate::authentication::Error::from)?;
    let upload_grant_decryptor = UploadGrantDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        "proofplane-document-upload-grant",
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(crate::authentication::Error::from)?;
    let upload_session_encryptor = UploadSessionEncryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        UPLOAD_SESSION_AUDIENCE,
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(crate::authentication::Error::from)?;
    let upload_session_decryptor = UploadSessionDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        UPLOAD_SESSION_AUDIENCE,
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(crate::authentication::Error::from)?;
    let document_upload_grant_service = DocumentUploadGrantService::new(
        dependencies.postgres.clone(),
        dependencies.config.server.public_api_base_url.clone(),
        upload_grant_encryptor,
        upload_grant_decryptor,
    );
    let upload_session_service =
        UploadSessionTokenService::new(upload_session_encryptor, upload_session_decryptor);
    let control_service = ControlService::new(dependencies.postgres.clone());
    let mcp_oauth_encryptor = McpOAuthEncryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        dependencies.config.mcp.resource.to_string(),
        &dependencies.config.paseto.mcp_oauth,
    )
    .map_err(crate::authentication::Error::from)?;
    let mcp_oauth_decryptor = McpOAuthDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        dependencies.config.mcp.resource.to_string(),
        &dependencies.config.paseto.mcp_oauth,
    )
    .map_err(crate::authentication::Error::from)?;
    let client_resolver =
        ClientResolver::from_mcp_oauth_config(&dependencies.config.paseto.mcp_oauth)
            .map_err(crate::authentication::Error::from)?;
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
    let auditor_grants = AuditorAccessGrantService::new(dependencies.postgres.clone());
    let auditor_sessions = AuditorAccessSessionService::new(
        dependencies.postgres.clone(),
        dependencies
            .mail_adapter
            .unwrap_or_else(|| crate::mailer::from_config(&dependencies.config.mail.adapter)),
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
        .merge(document_upload_sessions::router(
            DocumentUploadSessionState {
                grants: document_upload_grant_service,
                downloads: document_download_service.clone(),
                sessions: upload_session_service,
                submissions: evidence_submission_service,
                controls: control_service,
                secure_cookie: secure_upload_cookie,
                max_document_bytes: dependencies.config.uploads.max_document_bytes,
            },
        ))
        .merge(auditor_access::router(AuditorAccessState {
            grants: auditor_grants,
            sessions: auditor_sessions,
            portal: AuditorPortalReadModelService::new(dependencies.postgres.clone()),
            downloads: document_download_service,
            secure_cookie: secure_auditor_cookie,
        }))
        .merge(me::router(MeState {
            service: UserService::new(dependencies.postgres.clone()),
            route_auth: UserRouteAuthState {
                authenticator: dependencies.user_authenticator.clone(),
            },
        }))
        .merge(workspaces::router(WorkspacesState {
            service: WorkspaceService::new(dependencies.postgres.clone()),
            route_auth: UserRouteAuthState {
                authenticator: dependencies.user_authenticator.clone(),
            },
        }))
        .merge(agent_connections::router(AgentConnectionsState {
            service: AgentConnectionService::new(dependencies.postgres.clone()),
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
