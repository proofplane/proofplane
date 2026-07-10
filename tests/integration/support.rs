use std::{
    collections::HashMap,
    future::Future,
    io::{self, Write},
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex as StdMutex, OnceLock, Weak},
};

use async_trait::async_trait;
use axum::Router;
use axum_test::TestServer;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use futures_util::stream;
use metrics_exporter_prometheus::PrometheusBuilder;
use proofplane::services::{
    agent_connections::{digest_secret, AgentConnectionContext},
    evidence_requests::EvidenceRequestService,
    evidence_submissions::EvidenceSubmissionService,
};
use proofplane::{
    app::{create_app, AppDependencies},
    authentication::{
        auth0::{TokenVerifier, VerifiedClaims, VerifiedMcpClaims, VerifyError},
        paseto::{
            DownloadGrantDecryptor, DownloadGrantEncryptor, UploadGrantDecryptor,
            UploadGrantEncryptor,
        },
        UserAuthenticator,
    },
    config::{
        AppConfig, Auth0Config, Auth0UpstreamOAuthConfig, HealthConfig, LogFormat,
        MailAdapterConfig, MailConfig, McpConfig, ObjectStorageConfig, ObservabilityConfig,
        PasetoConfig, PasetoDownloadConfig, PasetoDownloadKey, PasetoMcpOAuthConfig,
        PasetoMcpOAuthKey, PasetoUploadGrantConfig, PasetoUploadGrantKey, PubSubConfig,
        PubSubSubscriptionsConfig, ScannerConfig, ServerConfig, UploadsConfig, WorkerConfig,
    },
    domain::{
        AgentAuthorizationTransactionId, AgentConnectionId, CreateEvidenceRequestPayload,
        CreateEvidenceSubmissionPayload, CreateWorkspacePayload, EvidenceRequestCadence,
        EvidenceRequestId, EvidenceRequestStatus, NewPendingAgentConnection, ProvisionUserPayload,
        UserId, WorkspaceId, WorkspacePermission, WorkspacePermissions, WorkspaceRole,
    },
    mailer::CapturingMailAdapter,
    mcp::{create_app as create_mcp_app, McpAppDependencies},
    repository::{NewWorkspaceMembership, Postgres},
    routes::authentication::AUTHORIZATION_HEADER,
    scanner::ClamAvMalwareScanner,
    store,
    worker::{create_worker_app, WorkerAppDependencies},
};
use secrecy::SecretString;
use serde_json::Value;
use testcontainers::{
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};
use testcontainers_modules::postgres;
use tokio::sync::{Mutex, OnceCell};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use uuid::Uuid;

const CLAMAV_IMAGE_TAG: &str = "1.4.3";
pub const INTEGRATION_API_TOKEN_ID: &str = "00000000-0000-4000-8000-000000000201";
// OnceCell provides one process-wide coordination slot; Mutex prevents concurrent
// starts; Weak permits reuse without keeping the container alive. Each TestApp holds
// a strong Arc, so the last app drop stops it and a later failed upgrade starts fresh.
static CLAMAV: OnceCell<Mutex<Weak<TestClamAv>>> = OnceCell::const_new();
static AUDIT_LOG_SINK: OnceLock<Arc<StdMutex<Vec<u8>>>> = OnceLock::new();

pub async fn upload_attachment(
    app: &TestApp,
    workspace_id: Uuid,
    submission_id: Uuid,
    filename: &str,
    content: &[u8],
) -> Value {
    let service = EvidenceSubmissionService::new(app.postgres_arc(), app.object_store.clone());
    let connection = app.agent_connection_context(workspace_id);
    let bytes = Bytes::copy_from_slice(content);
    let mut payload = service
        .upload_attachment(
            &connection,
            submission_id.into(),
            filename.to_owned(),
            "text/plain".to_owned(),
            stream::once(async move { Ok(bytes) }),
        )
        .await
        .expect("attachment object uploads");
    payload.checksum_crc32c = crc32c_base64(content);
    let attachment = service
        .create_attachment(&connection, Uuid::new_v4(), submission_id.into(), payload)
        .await
        .expect("attachment row creates");

    serde_json::json!({
        "id": Uuid::from(attachment.id),
        "evidence_submission_id": Uuid::from(attachment.evidence_submission_id),
        "filename": attachment.filename,
        "content_type": attachment.content_type,
        "content_length": attachment.content_length,
        "checksum_sha256": attachment.checksum_sha256,
        "checksum_crc32c": attachment.checksum_crc32c,
        "upload_status": attachment.upload_status.as_str(),
    })
}

// TODO: make this a method on TestApp that takes &Self
pub async fn capture_audit_logs<F, Fut, T>(capture: F) -> (T, Vec<Value>)
where
    F: FnOnce(Uuid) -> Fut,
    Fut: Future<Output = T>,
{
    let sink = audit_log_sink();
    let request_id = Uuid::new_v4();
    let start = sink.lock().expect("audit log sink locks").len();

    let output = capture(request_id).await;

    let request_id = request_id.to_string();
    let bytes = sink.lock().expect("audit log sink locks")[start..].to_vec();
    let records = String::from_utf8(bytes)
        .expect("audit logs are UTF-8")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| {
            record["fields"]["type"] == "audit_log"
                && record["fields"]["request_id"].as_str() == Some(request_id.as_str())
        })
        .collect();

    (output, records)
}

fn audit_log_sink() -> Arc<StdMutex<Vec<u8>>> {
    AUDIT_LOG_SINK
        .get_or_init(|| {
            let sink = Arc::new(StdMutex::new(Vec::new()));
            let subscriber = tracing_subscriber::fmt()
                .json()
                .with_current_span(false)
                .with_span_list(false)
                .with_file(false)
                .with_line_number(false)
                .with_writer(SharedWriter(sink.clone()))
                .with_env_filter(EnvFilter::new("proofplane::audit=info"))
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .expect("integration audit tracing subscriber installs");

            sink
        })
        .clone()
}

#[derive(Clone)]
struct SharedWriter(Arc<StdMutex<Vec<u8>>>);

impl<'a> MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriterGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriterGuard(self.0.clone())
    }
}

struct SharedWriterGuard(Arc<StdMutex<Vec<u8>>>);

impl Write for SharedWriterGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("audit log sink poisoned"))?
            .write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("audit log sink poisoned"))?
            .flush()
    }
}

/// Test double for the Auth0 token verifier. The bearer token IS the `auth0_sub`,
/// except for the reserved values below. Tokens prefixed with `noprofile:` omit the
/// `email`/`name` claims so JIT provisioning can be exercised without a profile.
pub struct FakeTokenVerifier;

#[async_trait]
impl TokenVerifier for FakeTokenVerifier {
    type Claims = VerifiedClaims;

    async fn verify(&self, token: &str) -> Result<VerifiedClaims, VerifyError> {
        if token.is_empty() || token == "invalid" {
            return Err(VerifyError::InvalidToken);
        }

        if let Some(sub) = token.strip_prefix("noprofile:") {
            return Ok(VerifiedClaims {
                sub: sub.to_owned(),
                email: None,
                name: None,
            });
        }

        Ok(VerifiedClaims {
            sub: token.to_owned(),
            email: Some(format!("{token}@example.com")),
            name: Some("Integration Human".to_owned()),
        })
    }
}

struct FakeMcpTokenVerifier;

#[async_trait]
impl TokenVerifier for FakeMcpTokenVerifier {
    type Claims = VerifiedMcpClaims;

    async fn verify(&self, token: &str) -> Result<VerifiedMcpClaims, VerifyError> {
        let parts = token.splitn(5, '|').collect::<Vec<_>>();
        if parts.len() != 5 || parts[0] != "test-oauth" {
            return Err(VerifyError::InvalidToken);
        }

        let connection_id = Uuid::parse_str(parts[1]).map_err(|_| VerifyError::InvalidToken)?;
        let workspace_id = Uuid::parse_str(parts[2]).map_err(|_| VerifyError::InvalidToken)?;
        let scopes = parts[3]
            .split(',')
            .filter(|scope| !scope.is_empty())
            .map(WorkspacePermission::from_str)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| VerifyError::InvalidScopes)?;

        Ok(VerifiedMcpClaims {
            subject: parts[4].to_owned(),
            client_id: format!("integration-client-{connection_id}"),
            scopes,
            connection_id: Some(connection_id.into()),
            workspace_id: Some(workspace_id.into()),
        })
    }
}

pub struct TestApp {
    // Dropping Testcontainers handles removes dependencies while the app still needs them.
    _postgres_container: ContainerAsync<postgres::Postgres>,
    _clamav: Option<Arc<TestClamAv>>,
    pub(super) postgres: Arc<Postgres>,
    app_config: AppConfig,
    object_store: Arc<proofplane::object_storage::FilesystemObjectStore>,
    object_storage_root: PathBuf,
    server: TestServer,
    api_token: String,
    api_token_id: Uuid,
    user_id: Uuid,
    home_workspace_id: Uuid,
    workspace_ids: HashMap<String, Uuid>,
    control_ids: HashMap<String, HashMap<String, Uuid>>,
    mailer: Arc<CapturingMailAdapter>,
}

impl TestApp {
    pub fn builder() -> TestAppBuilder {
        TestAppBuilder::default()
    }

    pub async fn start() -> Self {
        Self::builder().build().await
    }

    pub async fn start_without_default_auth() -> Self {
        Self::builder().without_default_auth().build().await
    }

    async fn start_with_builder(builder: TestAppBuilder) -> Self {
        let clamav = if builder.clamav {
            Some(shared_clamav().await)
        } else {
            None
        };

        // TODO(low priority): allow dependency containers to be toggled for health tests.
        let postgres_container = postgres::Postgres::default()
            .start()
            .await
            .expect("Postgres test container starts");
        let host = postgres_container
            .get_host()
            .await
            .expect("Postgres test container has a host");
        let port = postgres_container
            .get_host_port_ipv4(5432)
            .await
            .expect("Postgres test container exposes Postgres");
        let database_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

        let app_config = config(
            database_url.clone(),
            builder.max_attachment_bytes,
            builder.public_api_base_url,
        );

        let mut database = store::conn(&database_url)
            .await
            .expect("fixture database connection opens");
        store::migrate(&mut database)
            .await
            .expect("database migrations run");

        let pool = store::conn_pool(&database_url, 8)
            .await
            .expect("application Postgres pool opens");
        let postgres = Arc::new(Postgres::new(pool));
        let object_storage_root = match &app_config.object_storage {
            ObjectStorageConfig::Filesystem { root } => root.clone(),
            ObjectStorageConfig::Gcs(_) => unreachable!("integration tests use filesystem storage"),
        };
        let object_store = Arc::new(
            proofplane::object_storage::from_config(&app_config.object_storage)
                .await
                .expect("filesystem object store initializes"),
        );
        let recorder = PrometheusBuilder::new().build_recorder();
        let user_authenticator =
            UserAuthenticator::new(Arc::new(FakeTokenVerifier), postgres.clone());
        let mailer = Arc::new(CapturingMailAdapter::default());

        let dependencies = AppDependencies {
            config: app_config.clone(),
            postgres: postgres.clone(),
            object_store: object_store.clone(),
            metrics: recorder.handle(),
            user_authenticator,
            mail_adapter: Some(mailer.clone()),
        };

        let mut server = TestServer::new(create_app(dependencies).expect("app builds"));

        if builder.soc2_reference_data {
            insert_soc2_reference_data(&postgres).await;
        }

        let (workspace_ids, control_ids, home_workspace_id) =
            insert_workspaces(&postgres, builder.workspaces).await;
        // The integration API token belongs to exactly one workspace: the first
        // workspace granted default membership, or a dedicated home workspace
        // when a test declares none. Data-plane access to any other workspace
        // then 404s.
        let home_workspace_id = match home_workspace_id {
            Some(home_workspace_id) => home_workspace_id,
            None => create_workspace(&postgres, "Integration Home").await,
        };
        let issued = issue_api_token_for_workspace(
            &postgres,
            WorkspaceId::from(home_workspace_id),
            WorkspacePermission::ALL.to_vec(),
            app_config.mcp.resource.to_string(),
            Some(AgentConnectionId::from(
                Uuid::parse_str(INTEGRATION_API_TOKEN_ID).expect("integration token ID is a UUID"),
            )),
        )
        .await;
        if builder.default_auth {
            server.add_header(AUTHORIZATION_HEADER, format!("Bearer {}", issued.raw_token));
        }

        Self {
            _postgres_container: postgres_container,
            _clamav: clamav,
            postgres,
            app_config,
            object_store,
            object_storage_root,
            server,
            api_token: issued.raw_token,
            api_token_id: Uuid::from(issued.token_id),
            user_id: Uuid::from(issued.user_id),
            home_workspace_id,
            workspace_ids,
            control_ids,
            mailer,
        }
    }

    pub async fn create_evidence_request(&self, workspace_id: Uuid, body: &Value) -> Value {
        let request = EvidenceRequestService::new(self.postgres.clone())
            .create(
                self.agent_connection_context(workspace_id),
                CreateEvidenceRequestPayload {
                    title: string_field(body, "title"),
                    description: string_field(body, "description"),
                    collection_instructions: string_field(body, "collection_instructions"),
                    cadence: string_field(body, "cadence")
                        .parse::<EvidenceRequestCadence>()
                        .expect("fixture cadence parses"),
                    due_at: string_field(body, "due_at")
                        .parse()
                        .expect("fixture due_at parses"),
                    schedule_anchor_at: string_field(body, "schedule_anchor_at")
                        .parse()
                        .expect("fixture schedule_anchor_at parses"),
                    freshness_window_days: body["freshness_window_days"]
                        .as_i64()
                        .map(|value| i32::try_from(value).expect("freshness window fits i32")),
                    status: string_field(body, "status")
                        .parse::<EvidenceRequestStatus>()
                        .expect("fixture status parses"),
                },
            )
            .await
            .expect("evidence request fixture creates");

        serde_json::json!({
            "id": Uuid::from(request.id),
            "workspace_id": Uuid::from(request.workspace_id),
            "title": request.title,
            "description": request.description,
            "collection_instructions": request.collection_instructions,
            "cadence": request.cadence.as_str(),
            "due_at": request.due_at,
            "schedule_anchor_at": request.schedule_anchor_at,
            "freshness_window_days": request.freshness_window_days,
            "status": request.status.as_str(),
            "created_at": request.created_at,
            "updated_at": request.updated_at,
        })
    }

    pub async fn create_evidence_submission(
        &self,
        workspace_id: Uuid,
        evidence_request_id: Uuid,
        body: &Value,
    ) -> Value {
        let submission =
            EvidenceSubmissionService::new(self.postgres.clone(), self.object_store.clone())
                .create(
                    self.agent_connection_context(workspace_id),
                    EvidenceRequestId::from(evidence_request_id),
                    CreateEvidenceSubmissionPayload {
                        evidence_request_id: EvidenceRequestId::from(evidence_request_id),
                        coverage_start_at: string_field(body, "coverage_start_at")
                            .parse()
                            .expect("fixture coverage_start_at parses"),
                        coverage_end_at: string_field(body, "coverage_end_at")
                            .parse()
                            .expect("fixture coverage_end_at parses"),
                        source_system: string_field(body, "source_system"),
                        collection_method: string_field(body, "collection_method"),
                        summary: body["summary"].as_str().map(str::to_owned),
                        description: body["description"].as_str().map(str::to_owned),
                    },
                )
                .await
                .expect("evidence submission fixture creates")
                .expect("evidence submission fixture exists");

        serde_json::json!({
            "id": Uuid::from(submission.id),
            "evidence_request_id": Uuid::from(submission.evidence_request_id),
            "received_at": submission.received_at,
            "coverage_start_at": submission.coverage_start_at,
            "coverage_end_at": submission.coverage_end_at,
            "source_system": submission.source_system,
            "collection_method": submission.collection_method,
            "summary": submission.summary,
            "description": submission.description,
        })
    }

    pub fn agent_connection_context(&self, workspace_id: Uuid) -> AgentConnectionContext {
        AgentConnectionContext {
            user_id: self.user_id.into(),
            connection_id: self.api_token_id.into(),
            workspace_id: workspace_id.into(),
            permissions: WorkspacePermissions::from_iter(WorkspacePermission::ALL),
        }
    }

    /// Creates an API token bound to `workspace_id` with exactly `permissions`,
    /// returning the raw bearer token for explicit data-plane headers.
    pub async fn issue_api_token(
        &self,
        workspace_id: Uuid,
        permissions: Vec<WorkspacePermission>,
    ) -> IssuedTestApiToken {
        issue_api_token_for_workspace(
            &self.postgres,
            WorkspaceId::from(workspace_id),
            permissions,
            self.app_config.mcp.resource.to_string(),
            None,
        )
        .await
    }

    /// Inserts an evidence request directly, bypassing the API, so tests can seed
    /// a workspace the default API token is not scoped to.
    pub async fn insert_evidence_request_row(&self, workspace_id: Uuid, title: &str) {
        let client = self
            .postgres
            .get()
            .await
            .expect("evidence request fixture connection opens");
        client
            .execute(
                r#"
INSERT INTO evidence_requests (
    workspace_id, title, description, collection_instructions,
    cadence, due_at, schedule_anchor_at, freshness_window_days, status
)
VALUES ($1, $2, 'Seeded description', 'Seeded instructions', 'quarterly', now(), now(), 90, 'active')
"#,
                &[&workspace_id, &title],
            )
            .await
            .expect("evidence request fixture inserts");
    }

    pub fn server(&self) -> &TestServer {
        &self.server
    }

    pub fn postgres(&self) -> &Postgres {
        &self.postgres
    }

    pub fn postgres_arc(&self) -> Arc<Postgres> {
        self.postgres.clone()
    }

    /// Authenticates as `sub` through `GET /me`, which JIT-provisions the user,
    /// and returns the resulting user id.
    pub async fn login(&self, sub: &str) -> Uuid {
        let response = self
            .server
            .get("/me")
            .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
            .await;
        response.assert_status_ok();

        Uuid::parse_str(
            response.json::<Value>()["id"]
                .as_str()
                .expect("user id is a string"),
        )
        .expect("user id is a UUID")
    }

    pub async fn create_workspace_as(&self, sub: &str, name: &str) -> Value {
        let response = self
            .server
            .post("/workspace")
            .add_header(AUTHORIZATION_HEADER, format!("Bearer {sub}"))
            .json(&serde_json::json!({ "name": name }))
            .await;
        response.assert_status_ok();
        response.json()
    }

    pub async fn worker_server(&self) -> TestServer {
        let object_store = Arc::new(
            proofplane::object_storage::FilesystemObjectStore::new(&self.object_storage_root)
                .await
                .expect("worker filesystem object store initializes"),
        );
        let recorder = PrometheusBuilder::new().build_recorder();

        TestServer::new(create_worker_app(WorkerAppDependencies {
            postgres: self.postgres.clone(),
            object_store: object_store.clone(),
            scanner: Arc::new(ClamAvMalwareScanner::new(
                self.clamav_address(),
                std::time::Duration::from_secs(1),
                std::time::Duration::from_secs(30),
            )),
            worker_max_delivery_attempts: 5,
            metrics: recorder.handle(),
            live_path: "/livez".to_owned(),
            ready_path: "/readyz".to_owned(),
            dependency_timeout_ms: 1000,
        }))
    }

    pub fn object_storage_root(&self) -> &std::path::Path {
        &self.object_storage_root
    }

    pub fn clamav_address(&self) -> std::net::SocketAddr {
        self._clamav
            .as_ref()
            .expect("test app was built with ClamAV")
            .address
    }

    pub fn api_token(&self) -> &str {
        &self.api_token
    }

    pub fn api_token_id(&self) -> Uuid {
        self.api_token_id
    }

    pub fn home_workspace_id(&self) -> Uuid {
        self.home_workspace_id
    }

    pub fn user_id(&self) -> Uuid {
        self.user_id
    }

    pub fn bearer_token(&self) -> String {
        format!("Bearer {}", self.api_token)
    }

    pub fn sent_mail(&self) -> Vec<proofplane::mailer::CapturedOtp> {
        self.mailer.sent()
    }

    fn mcp_app(&self) -> Router {
        self.mcp_app_with_auth0_verifier(Arc::new(FakeMcpTokenVerifier))
    }

    fn mcp_app_with_auth0_verifier<V>(&self, auth0_verifier: Arc<V>) -> Router
    where
        V: TokenVerifier<Claims = VerifiedMcpClaims> + 'static,
    {
        let download_grant_encryptor = DownloadGrantEncryptor::from_config(
            self.app_config.server.public_api_base_url.clone(),
            "proofplane-attachment-download",
            &self.app_config.paseto.download,
        )
        .expect("download grant encryptor initializes");
        let download_grant_decryptor = DownloadGrantDecryptor::from_config(
            self.app_config.server.public_api_base_url.clone(),
            "proofplane-attachment-download",
            &self.app_config.paseto.download,
        )
        .expect("download grant decryptor initializes");
        let upload_grant_encryptor = UploadGrantEncryptor::from_config(
            self.app_config.server.public_api_base_url.clone(),
            "proofplane-attachment-upload-grant",
            &self.app_config.paseto.upload_grant,
        )
        .expect("upload grant encryptor initializes");
        let upload_grant_decryptor = UploadGrantDecryptor::from_config(
            self.app_config.server.public_api_base_url.clone(),
            "proofplane-attachment-upload-grant",
            &self.app_config.paseto.upload_grant,
        )
        .expect("upload grant decryptor initializes");
        let recorder = PrometheusBuilder::new().build_recorder();
        create_mcp_app(McpAppDependencies {
            postgres: self.postgres.clone(),
            object_store: self.object_store.clone(),
            metrics: recorder.handle(),
            oauth_verifier: auth0_verifier,
            authorization_server: self.app_config.server.public_api_base_url.clone(),
            resource: self.app_config.mcp.resource.clone(),
            public_api_base_url: self.app_config.server.public_api_base_url.clone(),
            download_grant_encryptor,
            download_grant_decryptor,
            upload_grant_encryptor,
            upload_grant_decryptor,
            health: HealthConfig {
                live_path: "/livez".to_owned(),
                ready_path: "/readyz".to_owned(),
                dependency_timeout_ms: 1000,
            },
            cancellation_token: CancellationToken::new(),
        })
        .expect("MCP application initializes")
    }

    pub fn mcp_http_server(&self) -> TestServer {
        TestServer::builder().http_transport().build(self.mcp_app())
    }

    pub fn mcp_http_server_with_auth0_verifier<V>(&self, verifier: Arc<V>) -> TestServer
    where
        V: TokenVerifier<Claims = VerifiedMcpClaims> + 'static,
    {
        TestServer::builder()
            .http_transport()
            .build(self.mcp_app_with_auth0_verifier(verifier))
    }

    pub fn workspace_id(&self, key: &str) -> Uuid {
        *self
            .workspace_ids
            .get(key)
            .unwrap_or_else(|| panic!("workspace fixture {key:?} exists"))
    }

    pub fn control_id(&self, workspace_key: &str, code: &str) -> Uuid {
        *self
            .control_ids
            .get(workspace_key)
            .unwrap_or_else(|| panic!("workspace fixture {workspace_key:?} exists"))
            .get(code)
            .unwrap_or_else(|| {
                panic!("control fixture {code:?} exists in workspace {workspace_key:?}")
            })
    }
}

pub struct TestAppBuilder {
    default_auth: bool,
    clamav: bool,
    soc2_reference_data: bool,
    workspaces: Vec<WorkspaceSpec>,
    max_attachment_bytes: usize,
    public_api_base_url: url::Url,
}

impl TestAppBuilder {
    pub fn without_default_auth(mut self) -> Self {
        self.default_auth = false;
        self
    }

    pub fn with_soc2_reference_data(mut self) -> Self {
        self.soc2_reference_data = true;
        self
    }

    pub fn with_clamav(mut self) -> Self {
        self.clamav = true;
        self
    }

    pub fn workspace(self, key: &'static str, name: &'static str) -> WorkspaceSpecBuilder {
        WorkspaceSpecBuilder {
            app: self,
            spec: WorkspaceSpec {
                key,
                name,
                default_membership: false,
                controls: Vec::new(),
            },
        }
    }

    pub async fn build(self) -> TestApp {
        TestApp::start_with_builder(self).await
    }
}

impl Default for TestAppBuilder {
    fn default() -> Self {
        Self {
            default_auth: true,
            clamav: false,
            soc2_reference_data: false,
            workspaces: Vec::new(),
            max_attachment_bytes: 25 * 1024 * 1024,
            public_api_base_url: url::Url::parse("https://api.proofplane.test/")
                .expect("public API base URL parses"),
        }
    }
}

pub struct WorkspaceSpecBuilder {
    app: TestAppBuilder,
    spec: WorkspaceSpec,
}

impl WorkspaceSpecBuilder {
    pub fn with_control(
        mut self,
        code: &'static str,
        title: &'static str,
        requirement_ids: Vec<Uuid>,
    ) -> Self {
        self.spec.controls.push(ControlSpec {
            code,
            title,
            requirement_ids,
        });
        self
    }

    pub fn with_default_membership(mut self) -> TestAppBuilder {
        self.spec.default_membership = true;
        self.app.workspaces.push(self.spec);
        self.app
    }

    pub fn without_membership(mut self) -> TestAppBuilder {
        self.spec.default_membership = false;
        self.app.workspaces.push(self.spec);
        self.app
    }
}

struct WorkspaceSpec {
    key: &'static str,
    name: &'static str,
    default_membership: bool,
    controls: Vec<ControlSpec>,
}

struct ControlSpec {
    code: &'static str,
    title: &'static str,
    requirement_ids: Vec<Uuid>,
}

async fn insert_workspaces(
    postgres: &Postgres,
    workspaces: Vec<WorkspaceSpec>,
) -> (
    HashMap<String, Uuid>,
    HashMap<String, HashMap<String, Uuid>>,
    Option<Uuid>,
) {
    let mut ids = HashMap::new();
    let mut control_ids = HashMap::new();
    let mut home_workspace_id = None;

    for workspace in workspaces {
        let id: Uuid = postgres
            .create_workspace(&CreateWorkspacePayload {
                id: None,
                slug: None,
                name: workspace.name.to_owned(),
            })
            .await
            .expect("workspace fixture inserts")
            .id
            .into();

        // The integration API token can belong to a single workspace, so the
        // first workspace granted default membership becomes its home.
        if workspace.default_membership && home_workspace_id.is_none() {
            home_workspace_id = Some(id);
        }

        let mut workspace_control_ids = HashMap::new();
        for control in workspace.controls {
            let control_id = insert_control(postgres, id, &control).await;
            let existing = workspace_control_ids.insert(control.code.to_owned(), control_id);
            assert!(
                existing.is_none(),
                "control fixture code {:?} is unique in workspace {:?}",
                control.code,
                workspace.key
            );
        }

        let existing = ids.insert(workspace.key.to_owned(), id);
        assert!(
            existing.is_none(),
            "workspace fixture key {:?} is unique",
            workspace.key
        );
        control_ids.insert(workspace.key.to_owned(), workspace_control_ids);
    }

    (ids, control_ids, home_workspace_id)
}

async fn create_workspace(postgres: &Postgres, name: &str) -> Uuid {
    postgres
        .create_workspace(&CreateWorkspacePayload {
            id: None,
            slug: None,
            name: name.to_owned(),
        })
        .await
        .expect("workspace fixture inserts")
        .id
        .into()
}

async fn insert_control(postgres: &Postgres, workspace_id: Uuid, control: &ControlSpec) -> Uuid {
    let mut client = postgres
        .get()
        .await
        .expect("control fixture connection opens");
    let transaction = client
        .transaction()
        .await
        .expect("control fixture transaction starts");
    let control_id = Uuid::new_v4();

    transaction
        .execute(
            r#"
INSERT INTO controls (id, workspace_id, code, title, description)
VALUES ($1, $2, $3, $4, $5)
"#,
            &[
                &control_id,
                &workspace_id,
                &control.code,
                &control.title,
                &format!("Control description for {}.", control.title),
            ],
        )
        .await
        .expect("control fixture inserts");

    for requirement_id in &control.requirement_ids {
        transaction
            .execute(
                r#"
INSERT INTO control_framework_requirement_mappings (control_id, framework_requirement_id)
VALUES ($1, $2)
"#,
                &[&control_id, requirement_id],
            )
            .await
            .expect("control requirement fixture inserts");
    }

    transaction
        .commit()
        .await
        .expect("control fixture transaction commits");

    control_id
}

async fn insert_soc2_reference_data(postgres: &Postgres) {
    let client = postgres
        .get()
        .await
        .expect("SOC 2 reference fixture connection opens");
    client
        .execute(
            r#"
INSERT INTO frameworks (id, code, name, description)
VALUES ($1, 'soc2', 'SOC 2', 'SOC 2 Trust Services Criteria.')
"#,
            &[&soc2_framework_id()],
        )
        .await
        .expect("SOC 2 framework fixture inserts");

    for (id, code, title) in [
        (cc61_id(), "CC6.1", "Logical access security"),
        (cc71_id(), "CC7.1", "System monitoring"),
    ] {
        client
            .execute(
                r#"
INSERT INTO framework_requirements (id, framework_id, code, title, description)
VALUES ($1, $2, $3, $4, 'Seeded SOC 2 requirement.')
"#,
                &[&id, &soc2_framework_id(), &code, &title],
            )
            .await
            .expect("SOC 2 requirement fixture inserts");
    }
}

pub struct IssuedTestApiToken {
    pub raw_token: String,
    pub token_id: AgentConnectionId,
    pub user_id: UserId,
}

async fn issue_api_token_for_workspace(
    postgres: &Postgres,
    workspace_id: WorkspaceId,
    permissions: Vec<WorkspacePermission>,
    resource: String,
    token_id: Option<AgentConnectionId>,
) -> IssuedTestApiToken {
    let user = postgres
        .upsert_user_by_auth0_sub(&ProvisionUserPayload {
            auth0_sub: format!("auth0|agent-connection-user-{}", Uuid::new_v4()),
            email: Some("agent-connection-user@example.com".to_owned()),
            name: Some("Agent Connection User".to_owned()),
        })
        .await
        .expect("agent connection user inserts");

    postgres
        .in_transaction(async |context| {
            context
                .insert_workspace_membership(&NewWorkspaceMembership {
                    user_id: user.id,
                    workspace_id,
                    role: WorkspaceRole::Owner,
                })
                .await?;
            Ok(())
        })
        .await
        .expect("agent connection user membership inserts");

    let token_id = token_id.unwrap_or_else(|| AgentConnectionId::from(Uuid::new_v4()));
    let continuation = format!("continuation-{token_id}");
    let nonce = format!("nonce-{token_id}");
    let auth0_client_id = format!("integration-client-{token_id}");
    postgres
        .create_pending_agent_connection(&NewPendingAgentConnection {
            id: token_id,
            transaction_id: AgentAuthorizationTransactionId::from(Uuid::new_v4()),
            user_id: user.id,
            workspace_id,
            auth0_subject: user.auth0_sub.clone(),
            auth0_client_id,
            client_display_name: "Integration MCP Client".to_owned(),
            resource,
            permissions: permissions.clone(),
            pending_expires_at: Utc::now() + chrono::Duration::days(30),
            continuation_digest: digest_secret(&continuation),
            nonce_digest: digest_secret(&nonce),
        })
        .await
        .expect("agent connection inserts");
    postgres
        .consume_agent_connection_continuation(digest_secret(&continuation), digest_secret(&nonce))
        .await
        .expect("agent connection continuation consumes")
        .expect("agent connection is authorized");
    let scopes = permissions
        .iter()
        .map(|permission| permission.as_str())
        .collect::<Vec<_>>()
        .join(",");

    IssuedTestApiToken {
        raw_token: format!(
            "test-oauth|{token_id}|{workspace_id}|{scopes}|{}",
            user.auth0_sub
        ),
        token_id,
        user_id: user.id,
    }
}

struct TestClamAv {
    _container: ContainerAsync<GenericImage>,
    address: std::net::SocketAddr,
}

async fn shared_clamav() -> Arc<TestClamAv> {
    let shared = CLAMAV
        .get_or_init(|| async { Mutex::new(Weak::new()) })
        .await;
    let mut shared = shared.lock().await;

    if let Some(clamav) = shared.upgrade() {
        return clamav;
    }

    let container = GenericImage::new("clamav/clamav-debian", CLAMAV_IMAGE_TAG)
        .with_exposed_port(3310.tcp())
        .with_wait_for(WaitFor::healthcheck())
        .with_env_var("CLAMAV_NO_FRESHCLAMD", "true")
        .start()
        .await
        .expect("ClamAV test container starts");
    let host = container
        .get_host()
        .await
        .expect("ClamAV test container has a host");
    let port = container
        .get_host_port_ipv4(3310)
        .await
        .expect("ClamAV test container exposes clamd");
    let addresses = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .expect("ClamAV test address resolves")
        .collect::<Vec<_>>();
    let address = addresses
        .iter()
        .copied()
        .find(std::net::SocketAddr::is_ipv4)
        .or_else(|| addresses.first().copied())
        .expect("ClamAV test address has a socket address");
    let clamav = Arc::new(TestClamAv {
        _container: container,
        address,
    });

    *shared = Arc::downgrade(&clamav);
    clamav
}

fn config(
    database_url: String,
    max_attachment_bytes: usize,
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
            issuer: url::Url::parse("https://proofplane-integration.us.auth0.com/")
                .expect("auth0 issuer parses"),
            audience: "https://api.proofplane.test".to_owned(),
            jwks_url: url::Url::parse(
                "https://proofplane-integration.us.auth0.com/.well-known/jwks.json",
            )
            .expect("auth0 jwks url parses"),
            upstream_oauth: Auth0UpstreamOAuthConfig {
                client_id: "integration-auth0-client".to_owned(),
                client_secret: SecretString::from("integration-auth0-secret"),
                callback_path: "/oauth/auth0/callback".to_owned(),
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
        object_storage: ObjectStorageConfig::Filesystem { root: storage_root },
        scanner: ScannerConfig {
            clamd_address: socket_addr("127.0.0.1:3310"),
            connection_timeout_ms: 1000,
            scan_timeout_ms: 30000,
        },
        uploads: UploadsConfig {
            max_attachment_bytes,
        },
        mail: MailConfig {
            adapter: MailAdapterConfig::Disabled,
        },
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

fn string_field(body: &Value, field: &str) -> String {
    body[field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} is a string"))
        .to_owned()
}

pub fn soc2_framework_id() -> Uuid {
    Uuid::parse_str("30000000-0000-4000-8000-000000000000").unwrap()
}

pub fn cc61_id() -> Uuid {
    Uuid::parse_str("30000000-0000-4000-8000-000000000001").unwrap()
}

pub fn cc71_id() -> Uuid {
    Uuid::parse_str("30000000-0000-4000-8000-000000000002").unwrap()
}

pub fn crc32c_base64(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(crc32c::crc32c(bytes).to_be_bytes())
}
