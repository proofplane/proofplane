//! Postgres fixtures for persistence tests.
//!
//! Persistence behavior no client can observe — workspace scoping, row locking,
//! transactional rollback — is covered here against a real Postgres. Docker
//! must be available to run these tests.

use std::{sync::OnceLock, time::Duration};

use testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt};
use testcontainers_modules::postgres;
use tokio::runtime::Runtime;
use uuid::Uuid;

use crate::domain::{AgentConnectionId, EvidenceId, PolicyId, UserId, WorkspaceId};

use super::{self as persistence, Postgres};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const TEMPLATE_DATABASE: &str = "proofplane_template";
/// Must track the Postgres image in `docker-compose.yml`, so these tests cover
/// the same major version the application runs against.
const POSTGRES_IMAGE_TAG: &str = "17-alpine";

struct SharedContainer {
    // Held so the container and the runtime owning its Docker client outlive
    // every test. `SHARED` is a static, so neither is ever dropped.
    _runtime: Runtime,
    _container: ContainerAsync<postgres::Postgres>,
    host: String,
    port: u16,
}

impl SharedContainer {
    fn url(&self, database: &str) -> String {
        format!(
            "postgres://postgres:postgres@{}:{}/{database}",
            self.host, self.port
        )
    }
}

static SHARED: OnceLock<SharedContainer> = OnceLock::new();

fn shared() -> &'static SharedContainer {
    SHARED.get_or_init(|| {
        // `#[tokio::test]` builds a fresh runtime per test, so the container
        // gets a thread and runtime of its own. One bound to a single test's
        // runtime would die when that test finished.
        std::thread::spawn(|| {
            let runtime = Runtime::new().expect("persistence test runtime builds");
            let (container, host, port) = runtime.block_on(async {
                let container = postgres::Postgres::default()
                    .with_tag(POSTGRES_IMAGE_TAG)
                    .with_startup_timeout(STARTUP_TIMEOUT)
                    .start()
                    .await
                    .expect("Postgres test container starts");
                let host = container
                    .get_host()
                    .await
                    .expect("Postgres test container has a host")
                    .to_string();
                let port = container
                    .get_host_port_ipv4(5432)
                    .await
                    .expect("Postgres test container exposes Postgres");

                let admin = persistence::conn(&admin_url(&host, port))
                    .await
                    .expect("administrative database connection opens");
                admin
                    .execute(&format!("CREATE DATABASE {TEMPLATE_DATABASE}"), &[])
                    .await
                    .expect("template database is created");

                let mut client = persistence::conn(&format!(
                    "postgres://postgres:postgres@{host}:{port}/{TEMPLATE_DATABASE}"
                ))
                .await
                .expect("template database connection opens");
                persistence::migrate(&mut client)
                    .await
                    .expect("migrations apply to the template database");
                drop(client);

                // Postgres refuses to clone a template that still has sessions
                // attached, and the migration connection closes asynchronously.
                admin
                    .execute(
                        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1",
                        &[&TEMPLATE_DATABASE],
                    )
                    .await
                    .expect("template database sessions close");

                (container, host, port)
            });

            SharedContainer {
                _runtime: runtime,
                _container: container,
                host,
                port,
            }
        })
        .join()
        .expect("Postgres test container thread completes")
    })
}

fn admin_url(host: &str, port: u16) -> String {
    format!("postgres://postgres:postgres@{host}:{port}/postgres")
}

/// A migrated, empty database of its own. Repository tests run in parallel, so
/// each one takes an isolated clone of the template migrated during startup.
pub async fn database() -> Postgres {
    let shared = shared();
    let name = format!("test_{}", Uuid::new_v4().simple());

    let admin = persistence::conn(&shared.url("postgres"))
        .await
        .expect("administrative database connection opens");
    admin
        .execute(
            &format!("CREATE DATABASE {name} TEMPLATE {TEMPLATE_DATABASE}"),
            &[],
        )
        .await
        .expect("test database clones the migrated template");

    let pool = persistence::conn_pool(&shared.url(&name), 4)
        .await
        .expect("test database pool opens");

    Postgres::new(pool)
}

/// A workspace with the user and agent connection that upload grants require.
pub struct TestWorkspace {
    pub workspace_id: WorkspaceId,
    pub user_id: UserId,
    pub agent_connection_id: AgentConnectionId,
}

/// Seeds a workspace, its owning user, and an active agent connection.
pub async fn workspace(postgres: &Postgres, name: &str) -> TestWorkspace {
    let client = postgres.get().await.expect("database connection opens");
    let workspace_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let agent_connection_id = Uuid::new_v4();

    client
        .execute(
            "INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3)",
            &[&workspace_id, &workspace_id.to_string(), &name],
        )
        .await
        .expect("workspace row inserts");
    client
        .execute(
            "INSERT INTO users (id, auth0_sub, email) VALUES ($1, $2, $3)",
            &[
                &user_id,
                &format!("auth0|{user_id}"),
                &format!("{user_id}@proofplane.test"),
            ],
        )
        .await
        .expect("user row inserts");
    client
        .execute(
            r#"
INSERT INTO agent_connections (
    id,
    user_id,
    workspace_id,
    auth0_subject,
    auth0_client_id,
    client_display_name,
    resource,
    status,
    pending_expires_at,
    activated_at
)
VALUES ($1, $2, $3, $4, 'test-client', 'Test Agent', 'https://api.proofplane.test/mcp',
        'active', now() + interval '1 hour', now())
"#,
            &[
                &agent_connection_id,
                &user_id,
                &workspace_id,
                &format!("auth0|{user_id}"),
            ],
        )
        .await
        .expect("agent connection row inserts");

    TestWorkspace {
        workspace_id: workspace_id.into(),
        user_id: user_id.into(),
        agent_connection_id: agent_connection_id.into(),
    }
}

/// Seeds a policy in the given workspace.
pub async fn policy(postgres: &Postgres, workspace_id: WorkspaceId, name: &str) -> PolicyId {
    let client = postgres.get().await.expect("database connection opens");
    let policy_id = Uuid::new_v4();

    client
        .execute(
            "INSERT INTO policies (id, workspace_id, name) VALUES ($1, $2, $3)",
            &[&policy_id, &Uuid::from(workspace_id), &name],
        )
        .await
        .expect("policy row inserts");

    policy_id.into()
}

/// Seeds an active evidence record in the given workspace.
pub async fn evidence(postgres: &Postgres, workspace_id: WorkspaceId, title: &str) -> EvidenceId {
    evidence_with_status(postgres, workspace_id, title, "active").await
}

/// Seeds an evidence record in a specific lifecycle status.
pub async fn evidence_with_status(
    postgres: &Postgres,
    workspace_id: WorkspaceId,
    title: &str,
    status: &str,
) -> EvidenceId {
    let client = postgres.get().await.expect("database connection opens");
    let evidence_id = Uuid::new_v4();

    client
        .execute(
            r#"
INSERT INTO evidence (id, workspace_id, title, description, collection_instructions, status)
VALUES ($1, $2, $3, 'Seeded description', 'Seeded instructions', $4)
"#,
            &[&evidence_id, &Uuid::from(workspace_id), &title, &status],
        )
        .await
        .expect("evidence row inserts");

    evidence_id.into()
}
