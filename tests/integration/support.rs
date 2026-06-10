use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Arc};

use api_keys_simplified::{Environment, ExposeSecret};
use async_trait::async_trait;
use axum_test::multipart::{MultipartForm, Part};
use axum_test::{TestRequest, TestServer};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use metrics_exporter_prometheus::PrometheusBuilder;
use proofplane::{
    app::{create_app, AppDependencies},
    authentication::{
        auth0::{TokenVerifier, VerifiedClaims, VerifyError},
        ApiKeyAuthenticator, ApiKeyManager, UserAuthenticator,
    },
    authorization::{spicedb::SpiceDbClient, workspaces::WorkspaceAuthorizer},
    config::{
        AppConfig, Auth0Config, HealthConfig, LogFormat, ObjectStorageConfig, ObservabilityConfig,
        PubSubConfig, PubSubSubscriptionsConfig, ServerConfig, SpiceDbConfig, UploadsConfig,
        WorkerConfig,
    },
    domain::{
        ActorId, ActorKind, CreateActorPayload, CreateApiCredentialPayload, CreateWorkspacePayload,
        WorkspaceId,
    },
    repository::Postgres,
    routes::authentication::{ACTOR_ID_HEADER, API_KEY_HEADER, AUTHORIZATION_HEADER},
    scanner::NoopMalwareScanner,
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
use uuid::Uuid;

const SPICEDB_PRESHARED_KEY: &str = "proofplane-integration-spicedb-key";
const SPICEDB_SCHEMA: &str = include_str!("../../authz/spicedb/proofplane.zed");
pub const INTEGRATION_ACTOR_ID: &str = "00000000-0000-4000-8000-000000000201";

/// Test double for the Auth0 token verifier. The bearer token IS the `auth0_sub`,
/// except for the reserved values below. Tokens prefixed with `noprofile:` omit the
/// `email`/`name` claims so JIT provisioning can be exercised without a profile.
pub struct FakeTokenVerifier;

#[async_trait]
impl TokenVerifier for FakeTokenVerifier {
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

pub struct TestApp {
    // Dropping Testcontainers handles removes dependencies while the app still needs them.
    _postgres_container: ContainerAsync<postgres::Postgres>,
    _spicedb_container: ContainerAsync<GenericImage>,
    pub(super) postgres: Arc<Postgres>,
    object_storage_root: PathBuf,
    server: TestServer,
    api_key: String,
    workspace_ids: HashMap<String, Uuid>,
    control_ids: HashMap<String, HashMap<String, Uuid>>,
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

        let spicedb_container = start_spicedb().await;
        let app_config = config(
            database_url.clone(),
            spicedb_endpoint(&spicedb_container).await,
            builder.max_attachment_bytes,
        );
        let spicedb = SpiceDbClient::from_config(&app_config.spicedb)
            .await
            .expect("SpiceDB client connects");
        spicedb
            .write_schema(SPICEDB_SCHEMA)
            .await
            .expect("SpiceDB schema applies");

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
        // It's safe to unwrap the result of returning the new ApiKeyManager because
        // we're in a test and it really shouldn't panic anyways.
        let authenticator =
            ApiKeyAuthenticator::new(ApiKeyManager::new().unwrap(), postgres.clone());
        let user_authenticator =
            UserAuthenticator::new(Arc::new(FakeTokenVerifier), postgres.clone());

        let dependencies = AppDependencies {
            config: app_config,
            postgres: postgres.clone(),
            object_store,
            metrics: recorder.handle(),
            authenticator,
            user_authenticator,
            workspace_authorizer: WorkspaceAuthorizer::new(spicedb.clone()),
        };

        let mut server = TestServer::new(create_app(dependencies).expect("app builds"));

        if builder.soc2_reference_data {
            insert_soc2_reference_data(&postgres).await;
        }

        let (workspace_ids, control_ids) =
            insert_workspaces(&postgres, &spicedb, builder.workspaces).await;
        let api_key = insert_api_credential(&postgres).await;
        if builder.default_auth {
            server.add_header(ACTOR_ID_HEADER, INTEGRATION_ACTOR_ID);
            server.add_header(API_KEY_HEADER, &api_key);
        }

        Self {
            _postgres_container: postgres_container,
            _spicedb_container: spicedb_container,
            postgres,
            object_storage_root,
            server,
            api_key,
            workspace_ids,
            control_ids,
        }
    }

    pub async fn create_evidence_request(&self, workspace_id: Uuid, body: &Value) -> Value {
        let response = self
            .server
            .post(&format!("/workspaces/{workspace_id}/evidence-requests"))
            .add_header(ACTOR_ID_HEADER, self.actor_id())
            .add_header(API_KEY_HEADER, self.api_key())
            .json(body)
            .await;

        response.assert_status_ok();
        response.json()
    }

    pub fn server(&self) -> &TestServer {
        &self.server
    }

    pub fn postgres(&self) -> &Postgres {
        &self.postgres
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
            .post("/workspaces")
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
            object_store,
            scanner: Arc::new(NoopMalwareScanner),
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

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn actor_id(&self) -> &str {
        INTEGRATION_ACTOR_ID
    }

    pub fn get(&self, path: &str) -> TestRequest {
        self.server
            .get(path)
            .add_header(ACTOR_ID_HEADER, self.actor_id())
            .add_header(API_KEY_HEADER, self.api_key())
    }

    pub fn post(&self, path: &str) -> TestRequest {
        self.server
            .post(path)
            .add_header(ACTOR_ID_HEADER, self.actor_id())
            .add_header(API_KEY_HEADER, self.api_key())
    }

    pub fn put(&self, path: &str) -> TestRequest {
        self.server
            .put(path)
            .add_header(ACTOR_ID_HEADER, self.actor_id())
            .add_header(API_KEY_HEADER, self.api_key())
    }

    pub fn delete(&self, path: &str) -> TestRequest {
        self.server
            .delete(path)
            .add_header(ACTOR_ID_HEADER, self.actor_id())
            .add_header(API_KEY_HEADER, self.api_key())
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
    soc2_reference_data: bool,
    workspaces: Vec<WorkspaceSpec>,
    max_attachment_bytes: usize,
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

    pub fn with_max_attachment_bytes(mut self, max_attachment_bytes: usize) -> Self {
        self.max_attachment_bytes = max_attachment_bytes;
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
            soc2_reference_data: false,
            workspaces: Vec::new(),
            max_attachment_bytes: 25 * 1024 * 1024,
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
    spicedb: &SpiceDbClient,
    workspaces: Vec<WorkspaceSpec>,
) -> (
    HashMap<String, Uuid>,
    HashMap<String, HashMap<String, Uuid>>,
) {
    let mut ids = HashMap::new();
    let mut control_ids = HashMap::new();

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

        if workspace.default_membership {
            spicedb
                .write_workspace_membership(WorkspaceId::from(id), INTEGRATION_ACTOR_ID)
                .await
                .expect("workspace fixture membership writes");
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

    (ids, control_ids)
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

async fn insert_api_credential(postgres: &Postgres) -> String {
    let issued = ApiKeyManager::new()
        .expect("API key manager builds")
        .issue(Environment::test())
        .expect("integration API key issues");
    let api_key = issued.raw_key.expose_secret().to_owned();
    let actor_id = integration_actor_id();

    postgres
        .create_actor(&CreateActorPayload {
            id: Some(actor_id),
            kind: ActorKind::System,
            display_name: "Integration System".to_owned(),
        })
        .await
        .expect("integration actor inserts");

    postgres
        .create_api_credential(&CreateApiCredentialPayload {
            id: "integration-api-key".to_owned(),
            actor_id,
            name: "Integration API Key".to_owned(),
            key_id: issued.key_id.clone(),
            credential_hash: issued.credential_hash.clone(),
            expires_at: None,
            revoked_at: None,
        })
        .await
        .expect("integration API credential inserts");

    api_key
}

fn integration_actor_id() -> ActorId {
    ActorId::from(Uuid::parse_str(INTEGRATION_ACTOR_ID).expect("integration actor ID is a UUID"))
}

async fn start_spicedb() -> ContainerAsync<GenericImage> {
    GenericImage::new("authzed/spicedb", "v1.53.0")
        .with_exposed_port(50051.tcp())
        .with_wait_for(WaitFor::seconds(2))
        .with_cmd(["serve-testing"])
        .start()
        .await
        .expect("SpiceDB test server starts")
}

async fn spicedb_endpoint(spicedb: &ContainerAsync<GenericImage>) -> url::Url {
    let host = spicedb
        .get_host()
        .await
        .expect("SpiceDB test server has a host");
    let port = spicedb
        .get_host_port_ipv4(50051)
        .await
        .expect("SpiceDB test server exposes gRPC");

    url::Url::parse(&format!("http://{host}:{port}")).expect("SpiceDB endpoint parses")
}

fn config(
    database_url: String,
    spicedb_endpoint: url::Url,
    max_attachment_bytes: usize,
) -> AppConfig {
    let storage_root =
        std::env::temp_dir().join(format!("proofplane-integration-storage-{}", Uuid::new_v4()));

    AppConfig {
        server: ServerConfig {
            api_bind: socket_addr("127.0.0.1:0"),
            worker_bind: socket_addr("127.0.0.1:0"),
            mcp_bind: socket_addr("127.0.0.1:0"),
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
        spicedb: SpiceDbConfig {
            endpoint: spicedb_endpoint,
            preshared_key: SecretString::from(SPICEDB_PRESHARED_KEY),
            schema_path: PathBuf::from("authz/spicedb/proofplane.zed"),
        },
        auth0: Auth0Config {
            issuer: url::Url::parse("https://proofplane-integration.us.auth0.com/")
                .expect("auth0 issuer parses"),
            audience: "https://api.proofplane.test".to_owned(),
            jwks_url: url::Url::parse(
                "https://proofplane-integration.us.auth0.com/.well-known/jwks.json",
            )
            .expect("auth0 jwks url parses"),
        },
        object_storage: ObjectStorageConfig::Filesystem { root: storage_root },
        uploads: UploadsConfig {
            max_attachment_bytes,
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

pub fn soc2_framework_id() -> Uuid {
    Uuid::parse_str("30000000-0000-4000-8000-000000000000").unwrap()
}

pub fn cc61_id() -> Uuid {
    Uuid::parse_str("30000000-0000-4000-8000-000000000001").unwrap()
}

pub fn cc71_id() -> Uuid {
    Uuid::parse_str("30000000-0000-4000-8000-000000000002").unwrap()
}

pub fn attachment_form(
    bytes: &[u8],
    filename: &str,
    content_type: &str,
    checksum: Option<String>,
) -> MultipartForm {
    MultipartForm::new().add_part(
        "file",
        file_part(
            bytes,
            filename,
            content_type,
            &format!(
                "crc32c=:{}:",
                checksum.unwrap_or_else(|| crc32c_base64(bytes))
            ),
        ),
    )
}

pub fn attachment_form_with_digest(
    bytes: &[u8],
    filename: &str,
    content_type: &str,
    content_digest: &str,
) -> MultipartForm {
    MultipartForm::new().add_part(
        "file",
        file_part(bytes, filename, content_type, content_digest),
    )
}

pub fn file_part(bytes: &[u8], filename: &str, content_type: &str, content_digest: &str) -> Part {
    Part::bytes(bytes.to_vec())
        .file_name(filename)
        .mime_type(content_type)
        .add_header("content-digest", content_digest)
}

pub fn content_digest_header(bytes: &[u8]) -> String {
    format!("crc32c=:{}:", crc32c_base64(bytes))
}

pub fn crc32c_base64(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(crc32c::crc32c(bytes).to_be_bytes())
}
