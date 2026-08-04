use crate::support::{
    audit_log::audit_log_sink,
    auth::FakeTokenVerifier,
    auth0::{self, FakeAuditorIdentityProvider},
    clamd::{ClamdControls, FakeClamd},
    pubsub::TestPubSub,
    reference_data,
    scenario::types::TestFramework,
    worker::{start_worker, PipelineControls, PipelineEvents, PushProxy},
};
use std::{
    future,
    sync::{mpsc, Arc, OnceLock},
    thread,
    time::Duration,
};

use crate::support::config::config;
use axum_test::TestServer;
use metrics_exporter_prometheus::PrometheusBuilder;
use proofplane::{
    app::AppDependencies,
    authentication::{
        paseto::{
            AgentEvidenceUploadGrantDecryptor, AgentEvidenceUploadGrantEncryptor,
            AgentPolicyDocumentUploadGrantDecryptor, AgentPolicyDocumentUploadGrantEncryptor,
            DownloadGrantDecryptor, DownloadGrantEncryptor, McpOAuthDecryptor, McpOAuthEncryptor,
            PolicyUploadGrantDecryptor, PolicyUploadGrantEncryptor, UploadGrantDecryptor,
            UploadGrantEncryptor,
        },
        UserAuthenticator,
    },
    config::AppConfig,
    dequeuer::{OutboxDequeuer, OutboxDequeuerConfig},
    mcp::{create_app as create_mcp_app, McpAppDependencies},
    object_storage::FilesystemObjectStore,
    pubsub::{ensure_worker_subscription, GoogleCloudPublisher, PUBSUB_EMULATOR_HOST},
    repository::Postgres,
    routes::authentication::AUTHORIZATION_HEADER,
    services::{
        agent_evidence_upload_grants::AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE,
        agent_policy_document_upload_grants::AGENT_POLICY_DOCUMENT_UPLOAD_GRANT_AUDIENCE,
        client_resolver::ClientResolver, oauth::OAuthService,
        policy_document_upload_grants::POLICY_UPLOAD_GRANT_AUDIENCE,
    },
};
use serde_json::Value;
use testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt};
use testcontainers_modules::postgres;
use tokio_util::sync::CancellationToken;

use proofplane::store;
use uuid::Uuid;

const MAX_DOCUMENT_BYTES: usize = 25 * 1024 * 1024;
const POSTGRES_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const DOWNLOAD_AUDIENCE: &str = "proofplane-document-download";
const UPLOAD_GRANT_AUDIENCE: &str = "proofplane-document-upload-grant";

struct Harness {
    // Never read: held so the container outlives the test binary.
    _postgres_container: ContainerAsync<postgres::Postgres>,
    _pubsub: TestPubSub,
    _clamd: FakeClamd,
    // The servers also retain these dependencies. Keeping them here makes the
    // suite runtime's ownership of all runtime-bound resources explicit.
    _postgres: Arc<Postgres>,
    _object_store: Arc<FilesystemObjectStore>,
    _worker: Arc<TestServer>,
    _proxy: PushProxy,
    _dequeuer: tokio::task::JoinHandle<()>,
    app: TestApp,
}

#[derive(Clone)]
pub struct TestApp {
    app_server: Arc<TestServer>,
    mcp_server: Arc<TestServer>,
    auditor_identity_provider: Arc<FakeAuditorIdentityProvider>,
    pipeline_events: PipelineEvents,
    pipeline_controls: PipelineControls,
    clamd_controls: ClamdControls,
    reference_data: Arc<[TestFramework]>,
}

static APP: OnceLock<TestApp> = OnceLock::new();

impl Harness {
    async fn start() -> Self {
        auth0::start();

        let container = postgres::Postgres::default()
            .with_startup_timeout(POSTGRES_STARTUP_TIMEOUT)
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

        let mut app_config = config(
            database_url.clone(),
            MAX_DOCUMENT_BYTES,
            url::Url::parse("https://api.proofplane.test/").expect("public API base URL parses"),
        );

        let pool = store::conn_pool(&database_url, 8)
            .await
            .expect("application Postgres pool opens");
        let postgres = Arc::new(Postgres::new(pool));
        let reference_data = reference_data::seed(&postgres).await;

        let object_store = Arc::new(
            proofplane::object_storage::from_config(&app_config.object_storage)
                .await
                .expect("filesystem object store initializes"),
        );
        let auditor_identity_provider = Arc::new(FakeAuditorIdentityProvider::default());
        let clamd = FakeClamd::start().await;
        let clamd_controls = clamd.controls();
        app_config.scanner.clamd_address = clamd.address();

        let worker = start_worker(&app_config, &postgres, &object_store);
        let pipeline_events = PipelineEvents::new();
        let pipeline_controls = PipelineControls::new();
        let proxy =
            PushProxy::start(&worker, pipeline_events.clone(), pipeline_controls.clone()).await;
        app_config.pubsub.subscriptions.worker_push_endpoint = proxy.container_endpoint();

        let pubsub = TestPubSub::start().await;
        std::env::set_var(PUBSUB_EMULATOR_HOST, pubsub.emulator_host());
        ensure_worker_subscription(
            &app_config.pubsub.project_id,
            &app_config.pubsub.subscriptions,
        )
        .await
        .expect("worker Pub/Sub subscription provisions");

        let publisher = GoogleCloudPublisher::new(app_config.pubsub.project_id.clone())
            .await
            .expect("Pub/Sub publisher initializes");
        let dequeuer_postgres = postgres.clone();
        let dequeuer = tokio::spawn(async move {
            let dequeuer_config = OutboxDequeuerConfig {
                poll_interval: Duration::from_millis(25),
                ..OutboxDequeuerConfig::default()
            };
            OutboxDequeuer::new(&dequeuer_postgres, &publisher, dequeuer_config)
                .run_until_cancelled(future::pending())
                .await
                .expect("suite outbox dequeuer runs");
        });

        Self {
            _postgres_container: container,
            _pubsub: pubsub,
            _clamd: clamd,
            _postgres: postgres.clone(),
            _object_store: object_store.clone(),
            _worker: Arc::new(worker),
            _proxy: proxy,
            _dequeuer: dequeuer,
            app: TestApp {
                app_server: Arc::new(build_app_server(
                    &app_config,
                    &postgres,
                    &object_store,
                    &auditor_identity_provider,
                )),
                mcp_server: Arc::new(build_mcp_server(&app_config, &postgres, &object_store)),
                auditor_identity_provider,
                pipeline_events,
                pipeline_controls,
                clamd_controls,
                reference_data: reference_data.into(),
            },
        }
    }
}

impl TestApp {
    pub fn app_server(&self) -> &TestServer {
        &self.app_server
    }

    pub fn mcp_server(&self) -> &TestServer {
        &self.mcp_server
    }

    pub fn auditor_identity_provider(&self) -> &FakeAuditorIdentityProvider {
        &self.auditor_identity_provider
    }

    pub fn pipeline_events(&self) -> &PipelineEvents {
        &self.pipeline_events
    }

    pub fn pipeline_controls(&self) -> &PipelineControls {
        &self.pipeline_controls
    }

    pub fn clamd_controls(&self) -> &ClamdControls {
        &self.clamd_controls
    }

    pub(super) fn reference_data(&self) -> &[TestFramework] {
        &self.reference_data
    }

    pub async fn capture_audit_logs<F, T>(&self, capture: F) -> (T, Vec<Value>)
    where
        F: AsyncFnOnce(Uuid) -> T,
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

    pub async fn login(&self, sub: &str) -> Uuid {
        let response = self
            .app_server
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
}

fn build_app_server(
    app_config: &AppConfig,
    postgres: &Arc<Postgres>,
    object_store: &Arc<FilesystemObjectStore>,
    auditor_identity_provider: &Arc<FakeAuditorIdentityProvider>,
) -> TestServer {
    let recorder = PrometheusBuilder::new().build_recorder();
    let dependencies = AppDependencies {
        config: app_config.clone(),
        postgres: postgres.clone(),
        object_store: object_store.clone(),
        metrics: recorder.handle(),
        user_authenticator: UserAuthenticator::new(Arc::new(FakeTokenVerifier), postgres.clone()),
        auditor_identity_provider: Some(auditor_identity_provider.clone()),
    };

    TestServer::builder()
        .http_transport()
        .build(proofplane::app::create_app(dependencies).expect("app builds"))
}

fn build_mcp_server(
    app_config: &AppConfig,
    postgres: &Arc<Postgres>,
    object_store: &Arc<FilesystemObjectStore>,
) -> TestServer {
    let issuer = app_config.server.public_api_base_url.clone();
    let resource = app_config.mcp.resource.clone();

    let client_resolver = ClientResolver::from_mcp_oauth_config(&app_config.paseto.mcp_oauth)
        .expect("MCP client resolver initializes");
    let oauth_service = OAuthService::new(
        postgres.clone(),
        issuer.clone(),
        resource.clone(),
        client_resolver,
        McpOAuthEncryptor::from_config(
            issuer.clone(),
            resource.to_string(),
            &app_config.paseto.mcp_oauth,
        )
        .expect("MCP OAuth encryptor initializes"),
        McpOAuthDecryptor::from_config(
            issuer.clone(),
            resource.to_string(),
            &app_config.paseto.mcp_oauth,
        )
        .expect("MCP OAuth decryptor initializes"),
    );

    let recorder = PrometheusBuilder::new().build_recorder();
    let app = create_mcp_app(McpAppDependencies {
        postgres: postgres.clone(),
        object_store: object_store.clone(),
        metrics: recorder.handle(),
        oauth_verifier: Arc::new(oauth_service),
        authorization_server: issuer.clone(),
        resource,
        public_api_base_url: issuer.clone(),
        download_grant_encryptor: DownloadGrantEncryptor::from_config(
            issuer.clone(),
            DOWNLOAD_AUDIENCE,
            &app_config.paseto.download,
        )
        .expect("download grant encryptor initializes"),
        download_grant_decryptor: DownloadGrantDecryptor::from_config(
            issuer.clone(),
            DOWNLOAD_AUDIENCE,
            &app_config.paseto.download,
        )
        .expect("download grant decryptor initializes"),
        upload_grant_encryptor: UploadGrantEncryptor::from_config(
            issuer.clone(),
            UPLOAD_GRANT_AUDIENCE,
            &app_config.paseto.upload_grant,
        )
        .expect("upload grant encryptor initializes"),
        upload_grant_decryptor: UploadGrantDecryptor::from_config(
            issuer.clone(),
            UPLOAD_GRANT_AUDIENCE,
            &app_config.paseto.upload_grant,
        )
        .expect("upload grant decryptor initializes"),
        agent_upload_grant_encryptor: AgentEvidenceUploadGrantEncryptor::from_config(
            issuer.clone(),
            AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE,
            &app_config.paseto.upload_grant,
        )
        .expect("agent evidence upload grant encryptor initializes"),
        agent_upload_grant_decryptor: AgentEvidenceUploadGrantDecryptor::from_config(
            issuer.clone(),
            AGENT_EVIDENCE_UPLOAD_GRANT_AUDIENCE,
            &app_config.paseto.upload_grant,
        )
        .expect("agent evidence upload grant decryptor initializes"),
        agent_policy_upload_grant_encryptor: AgentPolicyDocumentUploadGrantEncryptor::from_config(
            issuer.clone(),
            AGENT_POLICY_DOCUMENT_UPLOAD_GRANT_AUDIENCE,
            &app_config.paseto.upload_grant,
        )
        .expect("agent policy upload grant encryptor initializes"),
        agent_policy_upload_grant_decryptor: AgentPolicyDocumentUploadGrantDecryptor::from_config(
            issuer.clone(),
            AGENT_POLICY_DOCUMENT_UPLOAD_GRANT_AUDIENCE,
            &app_config.paseto.upload_grant,
        )
        .expect("agent policy upload grant decryptor initializes"),
        policy_upload_grant_encryptor: PolicyUploadGrantEncryptor::from_config(
            issuer.clone(),
            POLICY_UPLOAD_GRANT_AUDIENCE,
            &app_config.paseto.upload_grant,
        )
        .expect("policy upload grant encryptor initializes"),
        policy_upload_grant_decryptor: PolicyUploadGrantDecryptor::from_config(
            issuer,
            POLICY_UPLOAD_GRANT_AUDIENCE,
            &app_config.paseto.upload_grant,
        )
        .expect("policy upload grant decryptor initializes"),
        max_document_bytes: app_config.uploads.max_document_bytes as u64,
        health: app_config.health.clone(),
        // Empty means the transport applies no Host guard.
        allowed_hosts: Vec::new(),
        cancellation_token: CancellationToken::new(),
    })
    .expect("MCP application initializes");

    // The MCP client transport dials a real address, so this server cannot run
    // on the default mock transport.
    TestServer::builder().http_transport().build(app)
}

fn start_suite() -> TestApp {
    let (ready, started) = mpsc::sync_channel(1);

    thread::Builder::new()
        .name("integration-v2-suite".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("integration-v2-suite-worker")
                .build()
                .expect("integration-v2 suite runtime builds");

            runtime.block_on(async move {
                let environment = Harness::start().await;
                ready
                    .send(environment.app.clone())
                    .expect("integration-v2 harness receiver remains available");

                // The suite environment and its runtime-bound resources live
                // until the test process exits.
                future::pending::<()>().await;
                drop(environment);
            });
        })
        .expect("integration-v2 suite thread spawns");

    started
        .recv()
        .expect("integration-v2 suite thread stopped before initialization completed")
}

pub async fn app() -> TestApp {
    APP.get_or_init(start_suite).clone()
}
