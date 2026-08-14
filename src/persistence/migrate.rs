use std::time::Duration;

use refinery::{embed_migrations, Error, Report, Runner};
use tokio_postgres::Client;
use tracing::debug;

embed_migrations!("./migrations");

/// How long a migration waits for a lock before giving up.
///
/// Short on purpose. refinery takes no advisory lock of its own, so a run that
/// meets a conflicting session queues behind it on ordinary table locks — and
/// everything arriving after it queues too. Failing the job is recoverable;
/// holding the deploy open until Cloud Run's task timeout is not.
pub const MIGRATION_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

pub fn migration_runner() -> Runner {
    migrations::runner()
}

/// Bounds how long anything on `client` waits for a lock.
///
/// Session scope, so the transactions refinery opens on the same client inherit
/// it. That also means it only reaches the server on a direct connection, which
/// is what the migration command uses: a transaction pooler releases the server
/// connection when this statement ends and hands the next transaction a
/// different one.
pub async fn set_migration_lock_timeout(client: &Client) -> Result<(), tokio_postgres::Error> {
    client
        .batch_execute(&format!(
            "SET lock_timeout = '{}ms'",
            MIGRATION_LOCK_TIMEOUT.as_millis()
        ))
        .await
}

/// Applies every pending migration and reports what it applied.
///
/// Unbounded on locks. The migration command pairs this with
/// [`set_migration_lock_timeout`]; the seed command and the test harnesses run
/// against databases nothing else is holding.
pub async fn apply_migrations(client: &mut Client) -> Result<Report, Error> {
    let report = migration_runner().run_async(client).await?;

    debug!("done! ran {} migrations", report.applied_migrations().len());

    Ok(report)
}
