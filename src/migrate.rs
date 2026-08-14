//! The migration command.

use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use refinery::Report;
use secrecy::{ExposeSecret, SecretString};
use thiserror::Error;

use crate::{
    config::{self, ConfigError, PROOFPLANE_CONFIG},
    persistence,
};

/// Need this because we don't want to just set the database connection string
/// as an environment variable since that would leak it to anybody who could
/// see the running process's information wherever it's run (Cloud Run, etc.).
pub const DATABASE_URL_FILE: &str = "PROOFPLANE_MIGRATION_DATABASE_URL_FILE";

pub const DATABASE_URL: &str = "PROOFPLANE_MIGRATION_DATABASE_URL";

#[derive(Debug, Default)]
struct CredentialSources {
    url_file: Option<PathBuf>,
    inline_url: Option<String>,
    config_path: Option<PathBuf>,
}

impl CredentialSources {
    fn from_env() -> Self {
        Self::from_variables(|name| env::var_os(name))
    }

    fn from_variables(lookup: impl Fn(&str) -> Option<OsString>) -> Self {
        Self {
            url_file: lookup(DATABASE_URL_FILE).map(PathBuf::from),
            inline_url: lookup(DATABASE_URL).and_then(|url| url.into_string().ok()),
            config_path: lookup(PROOFPLANE_CONFIG).map(PathBuf::from),
        }
    }
}

#[derive(Debug, Error)]
pub enum CredentialError {
    #[error(
        "no source is set: set {DATABASE_URL_FILE} to a file holding the database URL, \
         {DATABASE_URL} to the URL itself, or {PROOFPLANE_CONFIG} to an application configuration file"
    )]
    NoSource,

    #[error("{DATABASE_URL_FILE} names {path}, which could not be read")]
    UnreadableFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{DATABASE_URL_FILE} names {path}, whose contents are not a database URL: {message}")]
    MalformedFile { path: PathBuf, message: String },

    #[error("{DATABASE_URL} is not a database URL: {message}")]
    MalformedInline { message: String },

    #[error("{PROOFPLANE_CONFIG} names {path}, which did not load")]
    Configuration {
        path: PathBuf,
        #[source]
        source: Box<ConfigError>,
    },
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not resolve the migration database credential")]
    Credential(#[from] CredentialError),

    #[error("could not connect to the database")]
    Connection(#[from] persistence::connection::Error),

    #[error("could not bound the migration lock timeout")]
    LockTimeout(#[source] tokio_postgres::Error),

    #[error("could not apply migrations")]
    Migrations(#[from] refinery::Error),
}

/// Applies every pending migration and reports what it applied.
pub async fn run() -> Result<Report, Error> {
    apply(&resolve(CredentialSources::from_env())?).await
}

/// This only uses one postgres connection with a lock timeout set.
async fn apply(url: &SecretString) -> Result<Report, Error> {
    let mut client = persistence::conn(url.expose_secret()).await?;

    persistence::set_migration_lock_timeout(&client)
        .await
        .map_err(Error::LockTimeout)?;

    Ok(persistence::apply_migrations(&mut client).await?)
}

/// The first source that is present, whether or not it turns out to hold a
/// usable credential.
///
/// A source that is set but broken fails here rather than falling through to
/// the next one.
fn resolve(sources: CredentialSources) -> Result<SecretString, CredentialError> {
    if let Some(path) = sources.url_file {
        return from_file(&path);
    }

    if let Some(url) = sources.inline_url {
        return database_url(&url).map_err(|message| CredentialError::MalformedInline { message });
    }

    if let Some(path) = sources.config_path {
        return from_configuration(&path);
    }

    Err(CredentialError::NoSource)
}

fn from_file(path: &Path) -> Result<SecretString, CredentialError> {
    let contents = fs::read_to_string(path).map_err(|source| CredentialError::UnreadableFile {
        path: path.to_path_buf(),
        source,
    })?;

    database_url(&contents).map_err(|message| CredentialError::MalformedFile {
        path: path.to_path_buf(),
        message,
    })
}

fn from_configuration(path: &Path) -> Result<SecretString, CredentialError> {
    config::load_from_path(path)
        .map(|config| config.database.url)
        .map_err(|source| CredentialError::Configuration {
            path: path.to_path_buf(),
            source: Box::new(source),
        })
}

/// Trimmed before validation, because a secret written from a file usually ends
/// with the newline an editor left behind, and a trailing newline lands inside
/// the database name rather than being rejected.
fn database_url(value: &str) -> Result<SecretString, String> {
    config::postgres_connection_string(SecretString::from(value.trim()))
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    use secrecy::{ExposeSecret, SecretString};
    use uuid::Uuid;

    use super::{
        apply, resolve, CredentialError, CredentialSources, Report, DATABASE_URL,
        DATABASE_URL_FILE, PROOFPLANE_CONFIG,
    };
    use crate::persistence::{self, test_support};

    /// Deliberately all-different, so a resolver that reads the wrong source
    /// cannot coincidentally return the right URL.
    const FILE_URL: &str = "postgres://migrator:secret@db.example:5432/from_file";
    const INLINE_URL: &str = "postgres://migrator:secret@db.example:5432/from_inline";
    /// The URL `config/local.yaml` carries, which is what the configuration
    /// fallback must produce.
    const CONFIGURED_URL: &str = "postgres://proofplane:proofplane@127.0.0.1:6432/proofplane";

    const LOCAL_CONFIG: &str = "config/local.yaml";

    static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Removes itself when the test ends, including when the test panics.
    struct TempFile {
        path: PathBuf,
    }

    impl TempFile {
        fn holding(contents: &str) -> Self {
            let path = env::temp_dir().join(format!(
                "proofplane-migrate-test-{}-{}",
                std::process::id(),
                TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&path, contents).expect("the temporary file is written");

            Self { path }
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn resolved(sources: CredentialSources) -> String {
        resolve(sources)
            .expect("the credential resolves")
            .expose_secret()
            .to_owned()
    }

    fn rejection(sources: CredentialSources) -> String {
        resolve(sources)
            .expect_err("the credential is rejected")
            .to_string()
    }

    #[test]
    fn every_variable_reaches_the_field_it_names() {
        let sources = CredentialSources::from_variables(|name| match name {
            DATABASE_URL_FILE => Some("/run/secrets/database-url".into()),
            DATABASE_URL => Some(INLINE_URL.into()),
            PROOFPLANE_CONFIG => Some(LOCAL_CONFIG.into()),
            _ => None,
        });

        assert_eq!(
            sources.url_file,
            Some(PathBuf::from("/run/secrets/database-url"))
        );
        assert_eq!(sources.inline_url, Some(INLINE_URL.to_owned()));
        assert_eq!(sources.config_path, Some(PathBuf::from(LOCAL_CONFIG)));
    }

    #[test]
    fn the_secret_file_outranks_the_inline_url_and_the_configuration() {
        let file = TempFile::holding(FILE_URL);

        let url = resolved(CredentialSources {
            url_file: Some(file.path.clone()),
            inline_url: Some(INLINE_URL.to_owned()),
            config_path: Some(LOCAL_CONFIG.into()),
        });

        assert_eq!(url, FILE_URL);
    }

    #[test]
    fn the_inline_url_outranks_the_configuration() {
        let url = resolved(CredentialSources {
            inline_url: Some(INLINE_URL.to_owned()),
            config_path: Some(LOCAL_CONFIG.into()),
            ..Default::default()
        });

        assert_eq!(url, INLINE_URL);
    }

    #[test]
    fn the_application_configuration_is_the_last_resort() {
        let url = resolved(CredentialSources {
            config_path: Some(LOCAL_CONFIG.into()),
            ..Default::default()
        });

        assert_eq!(url, CONFIGURED_URL);
    }

    #[test]
    fn a_secret_file_may_end_with_the_newline_an_editor_left_behind() {
        let file = TempFile::holding(&format!("{FILE_URL}\n"));

        let url = resolved(CredentialSources {
            url_file: Some(file.path.clone()),
            ..Default::default()
        });

        assert_eq!(url, FILE_URL);
    }

    #[test]
    fn a_broken_secret_file_does_not_fall_through_to_the_inline_url() {
        let file = TempFile::holding("");

        let error = resolve(CredentialSources {
            url_file: Some(file.path.clone()),
            inline_url: Some(INLINE_URL.to_owned()),
            config_path: Some(LOCAL_CONFIG.into()),
        })
        .expect_err("a broken higher-precedence source fails the run");

        assert!(
            matches!(error, CredentialError::MalformedFile { .. }),
            "expected the file to be blamed, got: {error}"
        );
    }

    #[test]
    fn a_missing_secret_file_names_its_variable_and_path() {
        let missing: PathBuf = env::temp_dir().join("proofplane-migrate-test-absent");
        let _ = fs::remove_file(&missing);

        let message = rejection(CredentialSources {
            url_file: Some(missing.clone()),
            ..Default::default()
        });

        assert_eq!(
            message,
            format!(
                "{DATABASE_URL_FILE} names {}, which could not be read",
                missing.display()
            )
        );
    }

    #[test]
    fn an_empty_secret_file_names_its_variable_and_path() {
        let file = TempFile::holding("   \n");

        let message = rejection(CredentialSources {
            url_file: Some(file.path.clone()),
            ..Default::default()
        });

        assert_eq!(
            message,
            format!(
                "{DATABASE_URL_FILE} names {}, whose contents are not a database URL: must not be empty",
                file.path.display()
            )
        );
    }

    #[test]
    fn a_secret_file_holding_something_other_than_a_url_names_its_variable_and_path() {
        let file = TempFile::holding("not-a-database-url");

        let message = rejection(CredentialSources {
            url_file: Some(file.path.clone()),
            ..Default::default()
        });

        assert_eq!(
            message,
            format!(
                "{DATABASE_URL_FILE} names {}, whose contents are not a database URL: \
                 must be a valid Postgres connection string",
                file.path.display()
            )
        );
    }

    #[test]
    fn a_malformed_inline_url_names_its_variable() {
        let message = rejection(CredentialSources {
            inline_url: Some("not-a-database-url".to_owned()),
            ..Default::default()
        });

        assert_eq!(
            message,
            format!(
                "{DATABASE_URL} is not a database URL: must be a valid Postgres connection string"
            )
        );
    }

    #[test]
    fn a_missing_configuration_file_names_its_variable_and_path() {
        let missing: PathBuf = env::temp_dir().join("proofplane-migrate-test-absent.yaml");
        let _ = fs::remove_file(&missing);

        let message = rejection(CredentialSources {
            config_path: Some(missing.clone()),
            ..Default::default()
        });

        assert_eq!(
            message,
            format!(
                "{PROOFPLANE_CONFIG} names {}, which did not load",
                missing.display()
            )
        );
    }

    /// The chain an operator reads is the command's only output, and the
    /// configuration path is its deepest. Each layer states its own part and
    /// leaves the cause to the layer below, so a chain printer repeats nothing.
    #[test]
    fn a_printed_chain_states_each_cause_once() {
        let missing: PathBuf = env::temp_dir().join("proofplane-migrate-test-unprintable.yaml");
        let _ = fs::remove_file(&missing);

        let unreadable = fs::read_to_string(&missing).expect_err("the file is missing");
        let error = resolve(CredentialSources {
            config_path: Some(missing.clone()),
            ..Default::default()
        })
        .expect_err("the credential is rejected");

        assert_eq!(
            format!("{:#}", anyhow::Error::new(error)),
            format!(
                "{PROOFPLANE_CONFIG} names {path}, which did not load: \
                 failed to read config file {path}: {unreadable}",
                path = missing.display()
            )
        );
    }

    #[test]
    fn no_source_at_all_names_every_variable_it_would_have_used() {
        let message = rejection(CredentialSources::default());

        assert_eq!(
            message,
            format!(
                "no source is set: set {DATABASE_URL_FILE} to a file holding the database URL, \
                 {DATABASE_URL} to the URL itself, or {PROOFPLANE_CONFIG} to an application configuration file"
            )
        );
    }

    /// refinery's own bookkeeping table. Every run reads it, so holding it is
    /// how one session blocks another's migration.
    const HISTORY_TABLE: &str = "refinery_schema_history";

    /// Generous next to the command's own lock timeout, so a bounded failure is
    /// unambiguous — and a run that ignores the bound fails this test rather
    /// than hanging the suite behind it.
    const BLOCKED_RUN_ALLOWANCE: Duration = Duration::from_secs(30);

    /// Every table the migrated schema leaves empty. `refinery_schema_history`
    /// is refinery's bookkeeping and `workspace_permissions` is a lookup the
    /// schema itself populates; a row anywhere else is seed data.
    const POPULATED_BY_THE_SCHEMA: [&str; 2] = [HISTORY_TABLE, "workspace_permissions"];

    /// An empty database on the fixture's container, so a run can be observed
    /// from nothing rather than from the fixture's already-migrated state.
    async fn empty_database(fixture: &test_support::TestDatabase) -> SecretString {
        let name = format!("migrate_{}", Uuid::new_v4().simple());
        persistence::conn(&fixture.url)
            .await
            .expect("the fixture container connection opens")
            .batch_execute(&format!("CREATE DATABASE {name}"))
            .await
            .expect("the empty database is created");

        let mut url = url::Url::parse(&fixture.url).expect("the fixture URL parses");
        url.set_path(&name);

        SecretString::from(url.to_string())
    }

    fn applied(report: &Report) -> Vec<String> {
        report
            .applied_migrations()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[tokio::test]
    async fn a_run_against_an_empty_database_applies_every_embedded_migration() {
        let fixture = test_support::database().await;

        let report = apply(&empty_database(&fixture).await)
            .await
            .expect("migrations apply");

        assert_eq!(
            applied(&report),
            persistence::migration_runner()
                .get_migrations()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn a_run_against_the_latest_revision_applies_nothing() {
        // The fixture arrives migrated, so this is the second run.
        let fixture = test_support::database().await;

        let report = apply(&SecretString::from(fixture.url.clone()))
            .await
            .expect("the repeat run succeeds");

        assert_eq!(applied(&report), Vec::<String>::new());
    }

    #[tokio::test]
    async fn a_run_writes_no_seed_data() {
        let fixture = test_support::database().await;
        let url = empty_database(&fixture).await;

        apply(&url).await.expect("migrations apply");

        let client = persistence::conn(url.expose_secret())
            .await
            .expect("the connection opens");
        // Derived from the schema rather than listed here, so a table added
        // later cannot quietly escape the assertion.
        let rows = client
            .query_typed(
                "SELECT c.relname::text, \
                        (xpath('/row/count/text()', \
                               query_to_xml(format('SELECT count(*) AS count FROM public.%I', c.relname), \
                                            false, true, '')))[1]::text::bigint \
                 FROM pg_class c \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = 'public' AND c.relkind = 'r' \
                 ORDER BY c.relname",
                &[],
            )
            .await
            .expect("the per-table row counts are available");

        let populated: Vec<String> = rows
            .iter()
            .filter(|row| row.get::<_, i64>(1) > 0)
            .map(|row| row.get::<_, String>(0))
            .collect();

        assert_eq!(populated, POPULATED_BY_THE_SCHEMA.to_vec());
    }

    #[tokio::test]
    async fn a_run_blocked_on_its_lock_gives_up_within_the_timeout() {
        let fixture = test_support::database().await;

        let blocker = persistence::conn(&fixture.url)
            .await
            .expect("the competing session connects");
        blocker
            .batch_execute(&format!(
                "BEGIN; LOCK TABLE {HISTORY_TABLE} IN ACCESS EXCLUSIVE MODE"
            ))
            .await
            .expect("the competing session takes the lock");

        let error = tokio::time::timeout(
            BLOCKED_RUN_ALLOWANCE,
            apply(&SecretString::from(fixture.url.clone())),
        )
        .await
        .expect("the blocked run gives up instead of waiting for the lock")
        .expect_err("a blocked run cannot migrate");

        let reason = format!("{:#}", anyhow::Error::new(error));
        assert!(
            reason.contains("canceling statement due to lock timeout"),
            "expected the lock timeout to be the reason, got: {reason}"
        );
    }
}
