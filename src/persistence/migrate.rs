use std::time::Duration;

use refinery::{embed_migrations, Error, Report, Runner};
use thiserror::Error as ThisError;
use tokio_postgres::error::SqlState;
use tokio_postgres::Client;
use tracing::{debug, warn};

embed_migrations!("./migrations");

/// The mark for a migration that makes it impossible to roll back.
const BREAKING_PREFIX: &str = "breaking_";

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

#[derive(Debug, ThisError)]
pub enum SchemaRevisionError {
    #[error(
        "database schema is behind this binary: found {applied} of {expected} expected migrations; \
         next expected {next}; run the `migrate` command before starting this service"
    )]
    Behind {
        applied: usize,
        expected: usize,
        next: String,
    },

    #[error(
        "database schema is ahead of this binary: {blocking} at position {position} is a \
         breaking migration, so this binary must not run against this database. Deploy a \
         binary that embeds that migration."
    )]
    Ahead { position: usize, blocking: String },

    #[error(
        "database schema history does not match this binary at position {position}: \
         expected {expected}, observed {observed}; start a binary whose embedded migrations \
         exactly match the database"
    )]
    Unknown {
        position: usize,
        expected: String,
        observed: String,
    },

    #[error("failed to read database schema history")]
    Database(#[from] tokio_postgres::Error),
}

#[derive(Debug, PartialEq, Eq)]
struct SchemaRevision {
    version: Option<String>,
    name: Option<String>,
    checksum: Option<String>,
}

impl std::fmt::Display for SchemaRevision {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let version = self.version.as_deref().unwrap_or("<null>");
        let name = self.name.as_deref().unwrap_or("<null>");
        let checksum = self.checksum.as_deref().unwrap_or("<null>");

        write!(formatter, "V{version}__{name} (checksum {checksum})")
    }
}

impl SchemaRevision {
    fn try_from_row(row: &tokio_postgres::Row) -> Result<Self, tokio_postgres::Error> {
        Ok(Self {
            version: row.try_get(0)?,
            name: row.try_get(1)?,
            checksum: row.try_get(2)?,
        })
    }
}

fn embedded_schema_history() -> Vec<SchemaRevision> {
    migration_runner()
        .get_migrations()
        .iter()
        .map(|migration| SchemaRevision {
            version: Some(migration.version().to_string()),
            name: Some(migration.name().to_owned()),
            checksum: Some(migration.checksum().to_string()),
        })
        .collect()
}

/// Whether `revision` refuses every release older than itself.
fn is_breaking(revision: &SchemaRevision) -> bool {
    revision
        .name
        .as_deref()
        .is_some_and(|name| name.starts_with(BREAKING_PREFIX))
}

/// Decides whether this binary may serve a database that has migrations it does not embed.
///
/// `embedded` is how many migrations this binary carries, and therefore where the
/// database's extra tail begins.
fn ahead(observed: &[SchemaRevision], embedded: usize) -> Result<(), SchemaRevisionError> {
    let tail = &observed[embedded..];

    if let Some((offset, blocking)) = tail
        .iter()
        .enumerate()
        .find(|(_, revision)| is_breaking(revision))
    {
        return Err(SchemaRevisionError::Ahead {
            position: embedded + offset + 1,
            blocking: blocking.to_string(),
        });
    }

    warn!(
        embedded,
        applied = observed.len(),
        "database schema is ahead of this binary by {} migration(s), which is expected only \
         after a rollback",
        tail.len()
    );

    Ok(())
}

fn behind(applied: usize, expected: &[SchemaRevision]) -> SchemaRevisionError {
    SchemaRevisionError::Behind {
        applied,
        expected: expected.len(),
        next: expected
            .get(applied)
            .map(ToString::to_string)
            .unwrap_or_else(|| "<no embedded migration>".to_owned()),
    }
}

/// Verifies that this binary can serve the database's migration history.
///
/// The history must match the migrations embedded in this binary, and may then run ahead
/// of them by additive migrations.
///
/// A database that is behind, or that diverges at any shared position, is always refused.
///
/// This is deliberately read-only. Runtime processes use it at startup and leave all
/// schema changes to the dedicated `migrate` command.
pub async fn check_schema_revision(client: &Client) -> Result<(), SchemaRevisionError> {
    const HISTORY_QUERY: &str = r#"
SELECT version::text, name, checksum
FROM refinery_schema_history
ORDER BY version ASC
"#;

    let expected = embedded_schema_history();
    let rows = match client.query_typed(HISTORY_QUERY, &[]).await {
        Ok(rows) => rows,
        Err(error) if error.code() == Some(&SqlState::UNDEFINED_TABLE) => {
            return Err(behind(0, &expected));
        }
        Err(error) => return Err(error.into()),
    };
    let observed = rows
        .iter()
        .map(SchemaRevision::try_from_row)
        .collect::<Result<Vec<_>, _>>()?;

    if observed == expected {
        return Ok(());
    }

    if observed.len() < expected.len() && observed == expected[..observed.len()] {
        return Err(behind(observed.len(), &expected));
    }

    if observed.len() > expected.len() && expected == observed[..expected.len()] {
        return ahead(&observed, expected.len());
    }

    let mismatch = observed
        .iter()
        .zip(&expected)
        .position(|(observed, expected)| observed != expected)
        .unwrap_or_else(|| observed.len().min(expected.len()));

    Err(SchemaRevisionError::Unknown {
        position: mismatch + 1,
        expected: expected
            .get(mismatch)
            .map(ToString::to_string)
            .unwrap_or_else(|| "<end of embedded history>".to_owned()),
        observed: observed
            .get(mismatch)
            .map(ToString::to_string)
            .unwrap_or_else(|| "<end of database history>".to_owned()),
    })
}

/// Bounds how long anything on `client` waits for a lock.
pub async fn set_migration_lock_timeout(client: &Client) -> Result<(), tokio_postgres::Error> {
    client
        .batch_execute(&format!(
            "SET lock_timeout = '{}ms'",
            MIGRATION_LOCK_TIMEOUT.as_millis()
        ))
        .await
}

/// Applies every pending migration and reports what it applied.
pub async fn apply_migrations(client: &mut Client) -> Result<Report, Error> {
    let report = migration_runner().run_async(client).await?;

    debug!("done! ran {} migrations", report.applied_migrations().len());

    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
    };

    use secrecy::{ExposeSecret, SecretString};
    use tracing_subscriber::fmt::MakeWriter;
    use uuid::Uuid;

    use super::{check_schema_revision, embedded_schema_history, Client, SchemaRevisionError};
    use crate::config::DatabaseTls;
    use crate::persistence::{self, param, test_support};

    /// The version directly after the last migration this binary embeds.
    fn next_version() -> i32 {
        embedded_schema_history()
            .last()
            .and_then(|revision| revision.version.as_deref())
            .expect("at least one migration is embedded")
            .parse::<i32>()
            .expect("embedded migration version is an integer")
            + 1
    }

    /// Captures what a subscriber writes, so a test can read the log a runtime emits.
    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl SharedWriter {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().expect("log buffer is not poisoned").clone())
                .expect("captured logs are UTF-8")
        }
    }

    impl<'a> MakeWriter<'a> for SharedWriter {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    impl Write for SharedWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("log buffer is not poisoned")
                .extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Records a migration this binary does not embed, standing in for a later release.
    async fn record_revision(client: &Client, version: i32, name: &str) {
        client
            .execute_typed(
                "INSERT INTO refinery_schema_history (version, name, applied_on, checksum) \
                 VALUES ($1, $2, 'future', $3)",
                &[param(&version), param(&name), param(&"123456789")],
            )
            .await
            .expect("history row inserts");
    }

    async fn empty_database(fixture: &test_support::TestDatabase) -> SecretString {
        let name = format!("schema_revision_{}", Uuid::new_v4().simple());
        persistence::conn(&fixture.url, DatabaseTls::Disable)
            .await
            .expect("fixture database connection opens")
            .batch_execute(&format!("CREATE DATABASE {name}"))
            .await
            .expect("empty database is created");

        let mut url = url::Url::parse(&fixture.url).expect("fixture database URL parses");
        url.set_path(&name);
        SecretString::from(url.to_string())
    }

    #[tokio::test]
    async fn exact_migrated_history_passes() {
        let database = test_support::database().await;
        let client = persistence::conn(&database.url, DatabaseTls::Disable)
            .await
            .expect("database connection opens");

        check_schema_revision(&client)
            .await
            .expect("the migrated history matches the binary");
    }

    #[tokio::test]
    async fn empty_database_is_behind_and_remains_unmodified() {
        let fixture = test_support::database().await;
        let url = empty_database(&fixture).await;
        let client = persistence::conn(url.expose_secret(), DatabaseTls::Disable)
            .await
            .expect("empty database connection opens");

        let error = check_schema_revision(&client)
            .await
            .expect_err("an empty database is behind");

        assert!(matches!(
            &error,
            SchemaRevisionError::Behind { applied: 0, .. }
        ));
        assert!(error.to_string().contains("`migrate` command"));
        let history_table: Option<String> = client
            .query_typed_one("SELECT to_regclass('refinery_schema_history')::text", &[])
            .await
            .expect("history-table lookup succeeds")
            .get(0);
        assert_eq!(
            history_table, None,
            "the check does not create the history table"
        );
    }

    #[tokio::test]
    async fn ahead_by_unmarked_migrations_is_compatible() {
        let database = test_support::database().await;
        let client = persistence::conn(&database.url, DatabaseTls::Disable)
            .await
            .expect("database connection opens");
        let version = next_version();
        record_revision(&client, version, "add_control_owner").await;
        record_revision(&client, version + 1, "add_control_owner_index").await;

        let logs = SharedWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(logs.clone())
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);

        check_schema_revision(&client)
            .await
            .expect("an earlier release keeps serving a schema expanded past it");

        drop(guard);
        let logged = logs.contents();
        assert!(
            logged.contains("WARN") && logged.contains("ahead of this binary by 2 migration"),
            "an operator sees the rollback state in the logs: {logged}"
        );
    }

    #[tokio::test]
    async fn ahead_by_a_breaking_migration_is_refused() {
        let database = test_support::database().await;
        let client = persistence::conn(&database.url, DatabaseTls::Disable)
            .await
            .expect("database connection opens");
        let version = next_version();
        record_revision(&client, version, "breaking_drop_control_owner").await;

        let error = check_schema_revision(&client)
            .await
            .expect_err("a breaking migration refuses an earlier release");
        let message = error.to_string();

        assert!(matches!(&error, SchemaRevisionError::Ahead { .. }));
        assert!(message.contains(&format!("V{version}__breaking_drop_control_owner")));
        assert!(message.contains("is a breaking migration"));
    }

    #[tokio::test]
    async fn a_breaking_migration_behind_an_unmarked_one_still_refuses() {
        let database = test_support::database().await;
        let client = persistence::conn(&database.url, DatabaseTls::Disable)
            .await
            .expect("database connection opens");
        let version = next_version();
        record_revision(&client, version, "add_control_owner").await;
        record_revision(&client, version + 1, "breaking_drop_control_owner").await;

        let error = check_schema_revision(&client)
            .await
            .expect_err("a breaking migration anywhere in the tail refuses an earlier release");
        let message = error.to_string();

        assert!(matches!(&error, SchemaRevisionError::Ahead { .. }));
        assert!(
            message.contains(&format!("V{}__breaking_drop_control_owner", version + 1)),
            "the message names the breaking migration, not the one before it: {message}"
        );
    }

    #[tokio::test]
    async fn divergent_name_and_checksum_are_unknown() {
        let database = test_support::database().await;
        let client = persistence::conn(&database.url, DatabaseTls::Disable)
            .await
            .expect("database connection opens");
        let expected = embedded_schema_history()
            .into_iter()
            .next()
            .expect("at least one migration is embedded");
        let version = expected
            .version
            .as_deref()
            .expect("embedded migration has a version")
            .parse::<i32>()
            .expect("embedded migration version is an integer");
        let expected_name = expected
            .name
            .as_deref()
            .expect("embedded migration has a name");
        let expected_checksum = expected
            .checksum
            .as_deref()
            .expect("embedded migration has a checksum");

        client
            .execute_typed(
                "UPDATE refinery_schema_history SET name = $1 WHERE version = $2",
                &[param(&"renamed_migration"), param(&version)],
            )
            .await
            .expect("history name is changed");
        assert!(matches!(
            check_schema_revision(&client).await,
            Err(SchemaRevisionError::Unknown { .. })
        ));

        client
            .execute_typed(
                "UPDATE refinery_schema_history SET name = $1, checksum = $2 WHERE version = $3",
                &[param(&expected_name), param(&"0"), param(&version)],
            )
            .await
            .expect("history checksum is changed");
        let error = check_schema_revision(&client)
            .await
            .expect_err("a divergent checksum is unknown");

        assert!(matches!(&error, SchemaRevisionError::Unknown { .. }));
        assert!(error.to_string().contains(expected_checksum));
        assert!(error.to_string().contains("checksum 0"));
    }
}
