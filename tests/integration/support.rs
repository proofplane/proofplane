use std::{path::PathBuf, str::FromStr, sync::Arc};

use axum_test::TestServer;
use metrics_exporter_prometheus::PrometheusBuilder;
use proofplane::{
    app::{create_app, AppDependencies},
    config::{
        AppConfig, AuthConfig, HealthConfig, HostPort, LogFormat, ObjectStorageConfig,
        ObservabilityConfig, PubSubConfig, PubSubSubscriptionsConfig, PubSubTopicsConfig,
        ServerConfig, WorkerConfig,
    },
    repository::Postgres,
    store,
};
use secrecy::SecretString;
use serde_json::Value;
use testcontainers::{runners::AsyncRunner, ContainerAsync};
use testcontainers_modules::postgres;
use tokio_postgres::Client;
use uuid::Uuid;

pub struct TestApp {
    // Dropping the Testcontainers handle removes Postgres while the test app still needs it.
    _postgres_container: ContainerAsync<postgres::Postgres>,
    database: Client,
    server: TestServer,
}

impl TestApp {
    pub async fn start() -> Self {
        let container = postgres::Postgres::default()
            .start()
            .await
            .expect("Postgres test container starts");
        let host = container
            .get_host()
            .await
            .expect("Postgres test container has a host");
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("Postgres test container exposes Postgres");
        let database_url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

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
            config: config(database_url),
            postgres: Arc::new(Postgres::new(pool)),
            metrics: recorder.handle(),
        };

        Self {
            _postgres_container: container,
            database,
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
}

fn config(database_url: String) -> AppConfig {
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
