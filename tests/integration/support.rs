use std::{path::PathBuf, str::FromStr, sync::Arc};

use api_keys_simplified::{Environment, ExposeSecret};
use axum_test::TestServer;
use metrics_exporter_prometheus::PrometheusBuilder;
use proofplane::{
    app::{create_app, AppDependencies},
    authentication::{ApiKeyAuthenticator, ApiKeyManager},
    authorization::spicedb::SpiceDbClient,
    config::{
        AppConfig, HealthConfig, HostPort, LogFormat, ObjectStorageConfig, ObservabilityConfig,
        PubSubConfig, PubSubSubscriptionsConfig, PubSubTopicsConfig, ServerConfig, SpiceDbConfig,
        WorkerConfig,
    },
    domain::{ActorKind, CreateActorPayload, CreateApiCredentialPayload, CreateWorkspacePayload},
    repository::Postgres,
    routes::authentication::{ACTOR_ID_HEADER, API_KEY_HEADER},
    store,
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
pub const INTEGRATION_ACTOR_ID: &str = "integration-system";

pub struct TestApp {
    // Dropping Testcontainers handles removes dependencies while the app still needs them.
    _postgres_container: ContainerAsync<postgres::Postgres>,
    _spicedb_container: ContainerAsync<GenericImage>,
    postgres: Arc<Postgres>,
    spicedb: SpiceDbClient,
    server: TestServer,
    api_key: String,
}

impl TestApp {
    pub async fn start() -> Self {
        Self::start_with_default_auth(true).await
    }

    pub async fn start_without_default_auth() -> Self {
        Self::start_with_default_auth(false).await
    }

    async fn start_with_default_auth(default_auth: bool) -> Self {
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
        let api_key = insert_api_credential(&postgres).await;
        let recorder = PrometheusBuilder::new().build_recorder();
        // It's safe to unwrap the result of returning the new ApiKeyManager because
        // we're in a test and it really shouldn't panic anyways.
        let authenticator =
            ApiKeyAuthenticator::new(ApiKeyManager::new().unwrap(), postgres.clone());

        let dependencies = AppDependencies {
            config: app_config,
            postgres: postgres.clone(),
            metrics: recorder.handle(),
            authenticator,
        };

        let mut server = TestServer::new(create_app(dependencies).expect("app builds"));
        if default_auth {
            server.add_header(ACTOR_ID_HEADER, INTEGRATION_ACTOR_ID);
            server.add_header(API_KEY_HEADER, &api_key);
        }

        Self {
            _postgres_container: postgres_container,
            _spicedb_container: spicedb_container,
            postgres,
            spicedb,
            server,
            api_key,
        }
    }

    pub async fn insert_workspace(&self, name: &str) -> Uuid {
        self.postgres
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

    pub async fn create_evidence_request(&self, workspace_id: Uuid, body: &Value) -> Value {
        let response = self
            .server
            .post(&format!("/workspaces/{workspace_id}/evidence-requests"))
            .json(body)
            .await;

        response.assert_status_ok();
        response.json()
    }

    pub fn server(&self) -> &TestServer {
        &self.server
    }

    pub fn spicedb(&self) -> &SpiceDbClient {
        &self.spicedb
    }

    pub fn postgres(&self) -> &Postgres {
        &self.postgres
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }
}

async fn insert_api_credential(postgres: &Postgres) -> String {
    let issued = ApiKeyManager::new()
        .expect("API key manager builds")
        .issue(Environment::test())
        .expect("integration API key issues");
    let api_key = issued.raw_key.expose_secret().to_owned();

    postgres
        .create_actor(&CreateActorPayload {
            id: INTEGRATION_ACTOR_ID.to_owned(),
            kind: ActorKind::System,
            display_name: "Integration System".to_owned(),
        })
        .await
        .expect("integration actor inserts");
    postgres
        .create_api_credential(&CreateApiCredentialPayload {
            id: "integration-api-key".to_owned(),
            actor_id: INTEGRATION_ACTOR_ID.to_owned(),
            name: "Integration API Key".to_owned(),
            key_id: issued.key_id,
            credential_hash: issued.credential_hash,
            expires_at: None,
            revoked_at: None,
        })
        .await
        .expect("integration API credential inserts");

    api_key
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

fn config(database_url: String, spicedb_endpoint: url::Url) -> AppConfig {
    AppConfig {
        server: ServerConfig {
            api_bind: socket_addr("127.0.0.1:0"),
            worker_bind: socket_addr("127.0.0.1:0"),
            mcp_bind: socket_addr("127.0.0.1:0"),
        },
        postgres: SecretString::from(database_url),
        pubsub: PubSubConfig {
            project_id: "integration-test".to_owned(),
            emulator_host: HostPort {
                host: "127.0.0.1".to_owned(),
                port: 1,
            },
            topics: PubSubTopicsConfig {
                outbox: "integration-outbox".to_owned(),
                dead_letter: "integration-dead-letter".to_owned(),
            },
            subscriptions: PubSubSubscriptionsConfig {
                worker: "integration-worker".to_owned(),
            },
        },
        spicedb: SpiceDbConfig {
            endpoint: spicedb_endpoint,
            preshared_key: SecretString::from(SPICEDB_PRESHARED_KEY),
            schema_path: PathBuf::from("authz/spicedb/proofplane.zed"),
        },
        object_storage: ObjectStorageConfig::Filesystem {
            root: PathBuf::from(".integration-storage"),
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
