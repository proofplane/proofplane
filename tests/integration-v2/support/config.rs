use std::str::FromStr;

use proofplane::config::{
    AppConfig, Auth0AuditorPortalConfig, Auth0Config, Auth0UpstreamOAuthConfig, HealthConfig,
    LogFormat, McpConfig, ObjectStorageConfig, ObservabilityConfig, PasetoConfig,
    PasetoDownloadConfig, PasetoDownloadKey, PasetoMcpOAuthConfig, PasetoMcpOAuthKey,
    PasetoUploadGrantConfig, PasetoUploadGrantKey, PubSubConfig, PubSubSubscriptionsConfig,
    ScannerConfig, ServerConfig, UploadsConfig, WorkerConfig, WorkspaceInvitationPasetoKey,
    WorkspaceInvitationsConfig,
};
use secrecy::SecretString;
use uuid::Uuid;

pub fn config(
    database_url: String,
    max_document_bytes: usize,
    public_api_base_url: url::Url,
) -> AppConfig {
    let storage_root =
        std::env::temp_dir().join(format!("proofplane-integration-storage-{}", Uuid::new_v4()));

    AppConfig {
        server: ServerConfig {
            api_bind: socket_addr("127.0.0.1:0"),
            worker_bind: socket_addr("127.0.0.1:0"),
            mcp_bind: socket_addr("127.0.0.1:0"),
            public_api_base_url,
        },
        postgres: SecretString::from(database_url),
        pubsub: PubSubConfig {
            project_id: "integration-test".to_owned(),
            subscriptions: PubSubSubscriptionsConfig {
                worker: "integration-worker".to_owned(),
                worker_push_endpoint: url::Url::parse("http://127.0.0.1:0/pubsub/messages")
                    .expect("worker push endpoint parses"),
                worker_max_delivery_attempts: 5,
            },
        },
        auth0: Auth0Config {
            // The upstream tenant is `support::auth0`, which the harness starts
            // before building the app. The trailing slash matters: the OAuth
            // callback resolves the token endpoint with `issuer.join(..)`.
            issuer: url::Url::parse("http://127.0.0.1:9099/").expect("auth0 issuer parses"),
            audience: "https://api.proofplane.test".to_owned(),
            jwks_url: url::Url::parse("http://127.0.0.1:9099/.well-known/jwks.json")
                .expect("auth0 jwks url parses"),
            upstream_oauth: Auth0UpstreamOAuthConfig {
                client_id: "integration-auth0-client".to_owned(),
                client_secret: SecretString::from("integration-auth0-secret"),
                callback_path: "/oauth/auth0/callback".to_owned(),
            },
            auditor_portal: Auth0AuditorPortalConfig {
                client_id: "integration-auditor-client".to_owned(),
                client_secret: SecretString::from("integration-auditor-secret"),
                callback_path: "/auditor-access/auth0/callback".to_owned(),
                callback_url: url::Url::parse(
                    "https://api.proofplane.test/auditor-access/auth0/callback",
                )
                .expect("auditor callback URL parses"),
                connection: "email".to_owned(),
                authorization_endpoint: url::Url::parse("https://auth.proofplane.test/authorize")
                    .expect("auditor authorization endpoint parses"),
                token_endpoint: url::Url::parse("https://auth.proofplane.test/oauth/token")
                    .expect("auditor token endpoint parses"),
            },
        },
        paseto: PasetoConfig {
            download: PasetoDownloadConfig {
                active_key_id: "integration-download-001".to_owned(),
                keys: vec![PasetoDownloadKey {
                    id: "integration-download-001".to_owned(),
                    secret: SecretString::from(
                        "k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs",
                    ),
                }],
            },
            upload_grant: PasetoUploadGrantConfig {
                active_key_id: "integration-upload-grant-001".to_owned(),
                keys: vec![PasetoUploadGrantKey {
                    id: "integration-upload-grant-001".to_owned(),
                    secret: SecretString::from(
                        "k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY",
                    ),
                }],
            },
            mcp_oauth: PasetoMcpOAuthConfig {
                active_key_id: "integration-mcp-oauth-001".to_owned(),
                keys: vec![PasetoMcpOAuthKey {
                    id: "integration-mcp-oauth-001".to_owned(),
                    secret: SecretString::from(
                        "k4.local.BMyQa9GmLofWmmvtYCedLfePwmuJsMgNn96nW1PtMp0",
                    ),
                }],
            },
        },
        workspace_invitations: WorkspaceInvitationsConfig {
            landing_portal_base_url: url::Url::parse("https://app.proofplane.test")
                .expect("landing portal URL parses"),
            active_key_id: "integration-workspace-invitation-001".to_owned(),
            keys: vec![WorkspaceInvitationPasetoKey {
                id: "integration-workspace-invitation-001".to_owned(),
                secret: SecretString::from("k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs"),
            }],
        },
        object_storage: ObjectStorageConfig::Filesystem { root: storage_root },
        scanner: ScannerConfig {
            clamd_address: socket_addr("127.0.0.1:3310"),
            connection_timeout_ms: 1000,
            scan_timeout_ms: 30000,
        },
        uploads: UploadsConfig { max_document_bytes },
        observability: ObservabilityConfig {
            log_format: LogFormat::Pretty,
            default_filter: "info".to_owned(),
        },
        worker: WorkerConfig {
            concurrency: 1,
            retry_attempts: 0,
            shutdown_grace_seconds: 1,
        },
        mcp: McpConfig {
            shutdown_grace_seconds: 1,
            resource: url::Url::parse("https://mcp.proofplane.test/mcp")
                .expect("MCP resource parses"),
            allowed_hosts: Vec::new(),
        },
        health: HealthConfig {
            live_path: "/livez".to_owned(),
            ready_path: "/readyz".to_owned(),
            dependency_timeout_ms: 1000,
        },
    }
}

fn socket_addr(value: &str) -> std::net::SocketAddr {
    std::net::SocketAddr::from_str(value).expect("test socket address parses")
}
