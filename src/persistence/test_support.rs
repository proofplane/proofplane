//! Postgres fixtures for persistence tests.
//!
//! Persistence behavior no client can observe — workspace scoping, row locking,
//! transactional rollback — is covered here against a real Postgres. Docker
//! must be available to run these tests.

use std::time::Duration;

use testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt};
use testcontainers_modules::postgres;
use uuid::Uuid;

use crate::config::DatabaseTlsConfig;
use crate::domain::{AgentConnectionId, EvidenceId, PolicyId, UserId, WorkspaceId};

use super::params::param;
use super::{self as persistence, Postgres};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
/// Small pool, generous timeouts: these tests care about persistence behavior,
/// not about pool bounds. Tests that do care build their own pool from
/// [`TestDatabase::url`].
const TEST_POOL_BOUNDS: persistence::PoolBounds =
    persistence::PoolBounds::new(4, Duration::from_secs(5), Duration::from_secs(300));
/// Must track the Postgres image in `docker-compose.yml`, so these tests cover
/// the same major version the application runs against.
const POSTGRES_IMAGE_TAG: &str = "17-alpine";

/// A migrated Postgres and the container serving it.
///
/// Move `postgres` out to use it; the container field stays behind and removes
/// the container when the test's binding goes out of scope.
pub struct TestDatabase {
    pub postgres: Postgres,
    /// The container's connection string, for tests that need a pool of their
    /// own rather than the shared one above.
    pub url: String,
    // Held so the container outlives the test; removed on drop.
    _container: ContainerAsync<postgres::Postgres>,
}

/// A migrated, empty database of its own, in a container of its own.
pub async fn database() -> TestDatabase {
    let container = postgres::Postgres::default()
        .with_tag(POSTGRES_IMAGE_TAG)
        .with_startup_timeout(STARTUP_TIMEOUT)
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
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    let mut client = persistence::conn(&url, &DatabaseTlsConfig::DISABLED)
        .await
        .expect("test database connection opens");
    persistence::apply_migrations(&mut client)
        .await
        .expect("migrations apply to the test database");
    drop(client);

    let pool = persistence::conn_pool(&url, &DatabaseTlsConfig::DISABLED, TEST_POOL_BOUNDS)
        .await
        .expect("test database pool opens");

    TestDatabase {
        postgres: Postgres::new(pool),
        url,
        _container: container,
    }
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
        .execute_typed(
            "INSERT INTO workspaces (id, slug, name) VALUES ($1, $2, $3)",
            &[
                param(&workspace_id),
                param(&workspace_id.to_string()),
                param(&name),
            ],
        )
        .await
        .expect("workspace row inserts");
    client
        .execute_typed(
            "INSERT INTO users (id, auth0_sub, email) VALUES ($1, $2, $3)",
            &[
                param(&user_id),
                param(&format!("auth0|{user_id}")),
                param(&format!("{user_id}@proofplane.test")),
            ],
        )
        .await
        .expect("user row inserts");
    client
        .execute_typed(
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
                param(&agent_connection_id),
                param(&user_id),
                param(&workspace_id),
                param(&format!("auth0|{user_id}")),
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
        .execute_typed(
            "INSERT INTO policies (id, workspace_id, name) VALUES ($1, $2, $3)",
            &[
                param(&policy_id),
                param(&Uuid::from(workspace_id)),
                param(&name),
            ],
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
        .execute_typed(
            r#"
INSERT INTO evidence (id, workspace_id, title, description, collection_instructions, status)
VALUES ($1, $2, $3, 'Seeded description', 'Seeded instructions', $4)
"#,
            &[
                param(&evidence_id),
                param(&Uuid::from(workspace_id)),
                param(&title),
                param(&status),
            ],
        )
        .await
        .expect("evidence row inserts");

    evidence_id.into()
}
