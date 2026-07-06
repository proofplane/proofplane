use std::{collections::HashSet, sync::Arc, time::Duration};

use axum::{extract::MatchedPath, http::Request, middleware, response::Response, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::Span;

use crate::{
    authentication::{
        auth0::{TokenVerifier, VerifiedClaims},
        auth0_redirect_token::RedirectTokenCodec,
        paseto::{
            DownloadGrantDecryptor, DownloadGrantEncryptor, UploadGrantDecryptor,
            UploadGrantEncryptor, UploadSessionDecryptor, UploadSessionEncryptor,
        },
        ApiTokenAuthenticator, UserAuthenticator,
    },
    config::AppConfig,
    object_storage::FilesystemObjectStore,
    repository::Postgres,
    routes::{
        agent_connection_consent::{self, AgentConnectionConsentState},
        api_tokens::{self, ApiTokensState},
        attachment_downloads::{self, AttachmentDownloadRouteAuthState, AttachmentDownloadState},
        attachment_upload_sessions::{self, AttachmentUploadSessionState},
        controls::{self, ControlRouteAuthState, ControlState},
        error::not_found,
        evidence_requests::{self, EvidenceRequestRouteAuthState, EvidenceRequestState},
        evidence_submissions::{self, EvidenceSubmissionRouteAuthState, EvidenceSubmissionState},
        health::{self, ReadyState},
        internal_agent_connections::{self, InternalAgentConnectionsState},
        me::{self, MeState, UserRouteAuthState},
        metrics::{self, MetricsState},
        request_context::attach_request_id,
        version,
        workspaces::{self, WorkspacesState},
    },
    services::{
        agent_connections::AgentConnectionService,
        api_tokens::ApiTokenService,
        attachment_downloads::AttachmentDownloadService,
        attachment_upload_grants::AttachmentUploadGrantService,
        controls::ControlService,
        evidence_requests::EvidenceRequestService,
        evidence_submissions::EvidenceSubmissionService,
        upload_sessions::{UploadSessionTokenService, UPLOAD_SESSION_AUDIENCE},
        user::UserService,
        workspaces::WorkspaceService,
    },
};

pub struct AppDependencies<V: TokenVerifier<Claims = VerifiedClaims>> {
    pub config: AppConfig,
    pub postgres: Arc<Postgres>,
    pub object_store: Arc<FilesystemObjectStore>,
    pub metrics: PrometheusHandle,
    pub api_token_authenticator: ApiTokenAuthenticator,
    pub user_authenticator: UserAuthenticator<V>,
}

pub fn create_app<V: TokenVerifier<Claims = VerifiedClaims> + 'static>(
    dependencies: AppDependencies<V>,
) -> Result<Router, crate::authentication::Error> {
    let api_token_authenticator = dependencies.api_token_authenticator.clone();
    let evidence_submission_service = EvidenceSubmissionService::new(
        dependencies.postgres.clone(),
        dependencies.object_store.clone(),
    );
    let download_grant_encryptor = DownloadGrantEncryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        "proofplane-attachment-download",
        &dependencies.config.paseto.download,
    )
    .map_err(crate::authentication::Error::from)?;
    let download_grant_decryptor = DownloadGrantDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        "proofplane-attachment-download",
        &dependencies.config.paseto.download,
    )
    .map_err(crate::authentication::Error::from)?;
    let attachment_download_service = AttachmentDownloadService::new(
        dependencies.postgres.clone(),
        dependencies.object_store.clone(),
        dependencies.config.server.public_api_base_url.clone(),
        download_grant_encryptor,
        download_grant_decryptor,
    );
    let upload_grant_encryptor = UploadGrantEncryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        "proofplane-attachment-upload-grant",
        &dependencies.config.paseto.upload_grant,
    )
    .map_err(crate::authentication::Error::from)?;
    let upload_grant_decryptor = UploadGrantDecryptor::from_config(
        dependencies.config.server.public_api_base_url.clone(),
        "proofplane-attachment-upload-grant",
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
    let attachment_upload_grant_service = AttachmentUploadGrantService::new(
        dependencies.postgres.clone(),
        dependencies.config.server.public_api_base_url.clone(),
        upload_grant_encryptor,
        upload_grant_decryptor,
    );
    let upload_session_service =
        UploadSessionTokenService::new(upload_session_encryptor, upload_session_decryptor);
    let secure_upload_cookie = dependencies.config.server.public_api_base_url.scheme() == "https";
    let consent_url = dependencies
        .config
        .server
        .public_api_base_url
        .join("agent-connections/consent")
        .expect("validated public API base URL accepts consent path");
    let auth0_continue_url = dependencies
        .config
        .auth0
        .issuer
        .join("continue")
        .expect("validated Auth0 issuer accepts continue path");
    let consent_token_codec = Arc::new(RedirectTokenCodec::new(
        dependencies.config.auth0.action.shared_secret.clone(),
        dependencies.config.auth0.issuer.to_string(),
        consent_url.to_string(),
    ));
    let agent_connection_service = AgentConnectionService::new(dependencies.postgres.clone());

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
        .merge(evidence_requests::router(EvidenceRequestState {
            service: EvidenceRequestService::new(dependencies.postgres.clone()),
            route_auth: EvidenceRequestRouteAuthState {
                authenticator: api_token_authenticator.clone(),
            },
        }))
        .merge(evidence_submissions::router(EvidenceSubmissionState {
            service: evidence_submission_service.clone(),
            max_attachment_bytes: dependencies.config.uploads.max_attachment_bytes,
            route_auth: EvidenceSubmissionRouteAuthState {
                authenticator: api_token_authenticator.clone(),
            },
        }))
        .merge(attachment_downloads::router(AttachmentDownloadState {
            service: attachment_download_service.clone(),
            route_auth: AttachmentDownloadRouteAuthState {
                authenticator: api_token_authenticator.clone(),
            },
        }))
        .merge(attachment_upload_sessions::router(
            AttachmentUploadSessionState {
                grants: attachment_upload_grant_service,
                downloads: attachment_download_service,
                sessions: upload_session_service,
                submissions: evidence_submission_service,
                secure_cookie: secure_upload_cookie,
                max_attachment_bytes: dependencies.config.uploads.max_attachment_bytes,
            },
        ))
        .merge(controls::router(ControlState {
            service: ControlService::new(dependencies.postgres.clone()),
            route_auth: ControlRouteAuthState {
                authenticator: api_token_authenticator,
            },
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
        .merge(api_tokens::router(ApiTokensState {
            service: ApiTokenService::new(dependencies.postgres.clone()),
            route_auth: UserRouteAuthState {
                authenticator: dependencies.user_authenticator.clone(),
            },
        }))
        .merge(agent_connection_consent::router(
            AgentConnectionConsentState {
                service: agent_connection_service.clone(),
                token_codec: consent_token_codec.clone(),
                result_signer: consent_token_codec,
                resource: dependencies.config.auth0.mcp.resource.to_string(),
                allowed_client_ids: dependencies
                    .config
                    .auth0
                    .mcp
                    .allowed_client_ids
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>(),
                auth0_continue_url,
            },
        ))
        .merge(internal_agent_connections::router(
            InternalAgentConnectionsState {
                service: agent_connection_service,
                action_shared_secret: dependencies.config.auth0.action.shared_secret.clone(),
            },
        ))
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
                        api_token_id = tracing::field::Empty,
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
            .uri("/attachment-downloads?token=secret-jwt")
            .body(Body::empty())
            .unwrap();

        assert_eq!(trace_path(&request), "/attachment-downloads");
    }
}
