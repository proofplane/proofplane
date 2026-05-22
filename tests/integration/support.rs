use std::{path::PathBuf, str::FromStr, sync::Arc};

use axum_test::TestServer;
use metrics_exporter_prometheus::PrometheusBuilder;
use proofplane::{
    app::{create_app, AppDependencies},
    authorization::spicedb::SpiceDbClient,
    config::{
        AppConfig, AuthConfig, HealthConfig, HostPort, LogFormat, ObjectStorageConfig,
        ObservabilityConfig, PubSubConfig, PubSubSubscriptionsConfig, PubSubTopicsConfig,
        ServerConfig, SpiceDbConfig, WorkerConfig,
    },
    repository::Postgres,
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
use tokio_postgres::Client;
use uuid::Uuid;

const SPICEDB_PRESHARED_KEY: &str = "proofplane-integration-spicedb-key";
const SPICEDB_SCHEMA: &str = include_str!("../../authz/spicedb/proofplane.zed");

pub struct TestApp {
    // Dropping Testcontainers handles removes dependencies while the app still needs them.
    _postgres_container: ContainerAsync<postgres::Postgres>,
    _spicedb_container: ContainerAsync<GenericImage>,
    database: Client,
    spicedb: SpiceDbClient,
    server: TestServer,
}

impl TestApp {
    pub async fn start() -> Self {
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
        let recorder = PrometheusBuilder::new().build_recorder();
        let dependencies = AppDependencies {
            config: app_config,
            postgres: Arc::new(Postgres::new(pool)),
            metrics: recorder.handle(),
        };

        Self {
            _postgres_container: postgres_container,
            _spicedb_container: spicedb_container,
            database,
            spicedb,
            server: TestServer::new(create_app(dependencies)),
        }
    }

    pub async fn insert_workspace(&self, name: &str) -> Uuid {
        self.database
            .query_one(
                "INSERT INTO workspaces (name) VALUES ($1) RETURNING id",
                &[&name],
            )
            .await
            .expect("workspace fixture inserts")
            .get("id")
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
        auth: AuthConfig {
            api_key_header: "x-proofplane-api-key".to_owned(),
            credential_hash_pepper: SecretString::from("integration-pepper"),
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
