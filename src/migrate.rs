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
    config::{self, ConfigError, DatabaseTls, DatabaseTlsConfig, PROOFPLANE_CONFIG},
    persistence,
};

/// Need this because we don't want to just set the database connection string
/// as an environment variable since that would leak it to anybody who could
/// see the running process's information wherever it's run (Cloud Run, etc.).
pub const DATABASE_URL_FILE: &str = "PROOFPLANE_MIGRATION_DATABASE_URL_FILE";

pub const DATABASE_URL: &str = "PROOFPLANE_MIGRATION_DATABASE_URL";

/// Selects the transport for a local run. The command reads no application
/// configuration in production. A verified connection is therefore the default,
/// and only this variable changes it.
pub const DATABASE_TLS: &str = "PROOFPLANE_MIGRATION_DATABASE_TLS";

/// One or more PEM certificates to trust in addition to the system certificate
/// store. Supabase issues a root of its own, and the command reads no
/// application configuration in production, so the certificate arrives here.
///
/// This needs no `_FILE` twin. [`DATABASE_URL_FILE`] exists because a URL is a
/// secret that must stay out of the process environment. A certificate is
/// public, so that reason does not apply.
pub const DATABASE_ROOT_CERTIFICATE: &str = "PROOFPLANE_MIGRATION_DATABASE_TLS_ROOT_CERTIFICATE";

#[derive(Debug, Default)]
struct CredentialSources {
    url_file: Option<PathBuf>,
    inline_url: Option<String>,
    config_path: Option<PathBuf>,
    tls: Option<String>,
    root_certificate: Option<String>,
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
            tls: lookup(DATABASE_TLS).and_then(|mode| mode.into_string().ok()),
            root_certificate: lookup(DATABASE_ROOT_CERTIFICATE)
                .and_then(|pem| pem.into_string().ok()),
        }
    }
}

/// A database URL and the transport that reaches it.
#[derive(Debug)]
struct Credential {
    url: SecretString,
    tls: DatabaseTlsConfig,
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

    #[error("{DATABASE_TLS} is not a TLS mode: {message}")]
    MalformedTls { message: String },

    #[error("{DATABASE_ROOT_CERTIFICATE} is not a certificate: {message}")]
    MalformedRootCertificate { message: String },

    /// A separate variant from the one above. The certificate is fine and the
    /// situation is not, so "is not a certificate" would be false here.
    ///
    /// Both variables are named because either one may be the mistake. The
    /// certificate can come from the configuration while [`DATABASE_TLS`]
    /// lowers the transport, in which case the certificate variable is not even
    /// set.
    #[error(
        "the transport is `disable`, which consults no certificate, but a root \
         certificate is set. Check {DATABASE_TLS} and {DATABASE_ROOT_CERTIFICATE}."
    )]
    UnusedRootCertificate,

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
    let credential = resolve(CredentialSources::from_env())?;

    apply(&credential.url, &credential.tls).await
}

/// This only uses one postgres connection with a lock timeout set.
async fn apply(url: &SecretString, tls: &DatabaseTlsConfig) -> Result<Report, Error> {
    let mut client = persistence::conn(url.expose_secret(), tls).await?;

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
///
/// The transport is verified unless [`DATABASE_TLS`] lowers it. The application
/// configuration is the one source that carries a mode of its own, so it
/// decides when the variable is absent.
fn resolve(sources: CredentialSources) -> Result<Credential, CredentialError> {
    let credential = select(sources)?;

    // A configuration that carries this pair already failed to load, so it
    // arrives only when a variable made it. Either variable may be the one at
    // fault, which is why the message names both.
    if credential.tls.mode == DatabaseTls::Disable && credential.tls.root_certificate.is_some() {
        return Err(CredentialError::UnusedRootCertificate);
    }

    Ok(credential)
}

/// Takes the first source that is present, and the transport that goes with it.
fn select(sources: CredentialSources) -> Result<Credential, CredentialError> {
    let requested_mode = requested_tls(sources.tls.as_deref())?;
    let requested_certificate = requested_root_certificate(sources.root_certificate)?;
    // Neither of the first two sources carries a transport of its own, so both
    // take this one. Only one branch below runs, so both may take it by value.
    let unstated = DatabaseTlsConfig {
        mode: requested_mode.unwrap_or(DatabaseTls::VerifyFull),
        root_certificate: requested_certificate.clone(),
    };

    if let Some(path) = sources.url_file {
        return Ok(Credential {
            url: from_file(&path)?,
            tls: unstated,
        });
    }

    if let Some(url) = sources.inline_url {
        let url =
            database_url(&url).map_err(|message| CredentialError::MalformedInline { message })?;

        return Ok(Credential { url, tls: unstated });
    }

    if let Some(path) = sources.config_path {
        let configured = from_configuration(&path)?;

        return Ok(Credential {
            tls: DatabaseTlsConfig {
                mode: requested_mode.unwrap_or(configured.tls.mode),
                // The variable adds or replaces a root. It cannot remove one:
                // an empty value is refused as malformed, and a root is removed
                // by editing the configuration.
                root_certificate: requested_certificate.or(configured.tls.root_certificate),
            },
            url: configured.url,
        });
    }

    Err(CredentialError::NoSource)
}

fn requested_tls(value: Option<&str>) -> Result<Option<DatabaseTls>, CredentialError> {
    value
        .map(|mode| {
            config::database_tls(mode.to_owned())
                .map_err(|message| CredentialError::MalformedTls { message })
        })
        .transpose()
}

fn requested_root_certificate(value: Option<String>) -> Result<Option<String>, CredentialError> {
    value
        .map(|pem| {
            config::database_root_certificate(pem)
                .map_err(|message| CredentialError::MalformedRootCertificate { message })
        })
        .transpose()
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

fn from_configuration(path: &Path) -> Result<Credential, CredentialError> {
    config::load_from_path(path)
        .map(|config| Credential {
            url: config.database.url,
            tls: config.database.tls,
        })
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
        apply, resolve, CredentialError, CredentialSources, DatabaseTls, DatabaseTlsConfig, Report,
        DATABASE_ROOT_CERTIFICATE, DATABASE_TLS, DATABASE_URL, DATABASE_URL_FILE,
        PROOFPLANE_CONFIG,
    };
    use crate::config::test_support::ROOT_CERTIFICATE;
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
            .url
            .expose_secret()
            .to_owned()
    }

    fn resolved_tls(sources: CredentialSources) -> DatabaseTlsConfig {
        resolve(sources).expect("the credential resolves").tls
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
            DATABASE_TLS => Some("disable".into()),
            DATABASE_ROOT_CERTIFICATE => Some(ROOT_CERTIFICATE.into()),
            _ => None,
        });

        assert_eq!(
            sources.url_file,
            Some(PathBuf::from("/run/secrets/database-url"))
        );
        assert_eq!(sources.inline_url, Some(INLINE_URL.to_owned()));
        assert_eq!(sources.config_path, Some(PathBuf::from(LOCAL_CONFIG)));
        assert_eq!(sources.tls, Some("disable".to_owned()));
        assert_eq!(sources.root_certificate, Some(ROOT_CERTIFICATE.to_owned()));
    }

    #[test]
    fn the_secret_file_outranks_the_inline_url_and_the_configuration() {
        let file = TempFile::holding(FILE_URL);

        let url = resolved(CredentialSources {
            url_file: Some(file.path.clone()),
            inline_url: Some(INLINE_URL.to_owned()),
            config_path: Some(LOCAL_CONFIG.into()),
            ..Default::default()
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

    /// The deployment sets one variable, and it names the file. Nothing else
    /// tells the command what the transport should be, so a verified one is
    /// what it must choose.
    #[test]
    fn the_secret_file_verifies_the_transport_by_default() {
        let file = TempFile::holding(FILE_URL);

        let tls = resolved_tls(CredentialSources {
            url_file: Some(file.path.clone()),
            ..Default::default()
        });

        assert_eq!(tls.mode, DatabaseTls::VerifyFull);
    }

    #[test]
    fn the_inline_url_verifies_the_transport_by_default() {
        let tls = resolved_tls(CredentialSources {
            inline_url: Some(INLINE_URL.to_owned()),
            ..Default::default()
        });

        assert_eq!(tls.mode, DatabaseTls::VerifyFull);
    }

    /// The one source that carries a mode of its own. A migration that reads an
    /// application configuration matches the runtimes that configuration will
    /// start.
    #[test]
    fn the_application_configuration_supplies_its_own_transport() {
        let tls = resolved_tls(CredentialSources {
            config_path: Some(LOCAL_CONFIG.into()),
            ..Default::default()
        });

        assert_eq!(tls, DatabaseTlsConfig::DISABLED);
    }

    /// What `make migrate` sets. The local stack serves no certificate, so the
    /// default has to be lowered from outside the command.
    #[test]
    fn the_tls_variable_lowers_every_source() {
        let file = TempFile::holding(FILE_URL);

        for sources in [
            CredentialSources {
                url_file: Some(file.path.clone()),
                tls: Some("disable".to_owned()),
                ..Default::default()
            },
            CredentialSources {
                inline_url: Some(INLINE_URL.to_owned()),
                tls: Some("disable".to_owned()),
                ..Default::default()
            },
        ] {
            assert_eq!(resolved_tls(sources), DatabaseTlsConfig::DISABLED);
        }
    }

    /// The variable also names the verifying mode. An operator can therefore
    /// state the transport, and not only lower it.
    #[test]
    fn the_tls_variable_also_raises_the_configuration() {
        let tls = resolved_tls(CredentialSources {
            config_path: Some(LOCAL_CONFIG.into()),
            tls: Some("verify-full".to_owned()),
            ..Default::default()
        });

        assert_eq!(tls.mode, DatabaseTls::VerifyFull);
    }

    /// No source but the application configuration carries a certificate, and
    /// production mounts no configuration. The variable is the only way a root
    /// reaches the migration job.
    #[test]
    fn the_root_certificate_variable_reaches_every_source() {
        let file = TempFile::holding(FILE_URL);

        for sources in [
            CredentialSources {
                url_file: Some(file.path.clone()),
                root_certificate: Some(ROOT_CERTIFICATE.to_owned()),
                ..Default::default()
            },
            CredentialSources {
                inline_url: Some(INLINE_URL.to_owned()),
                root_certificate: Some(ROOT_CERTIFICATE.to_owned()),
                ..Default::default()
            },
        ] {
            let tls = resolved_tls(sources);

            assert_eq!(tls.mode, DatabaseTls::VerifyFull);
            // Trimmed, like every other validated value.
            assert_eq!(
                tls.root_certificate.as_deref(),
                Some(ROOT_CERTIFICATE.trim())
            );
        }
    }

    /// The local configuration sets no certificate, so this also proves the
    /// variable supplies one where the configuration supplies none.
    #[test]
    fn the_root_certificate_variable_outranks_the_configuration() {
        let tls = resolved_tls(CredentialSources {
            config_path: Some(LOCAL_CONFIG.into()),
            // The local configuration is `disable`, which admits no
            // certificate, so the mode has to be raised with it.
            tls: Some("verify-full".to_owned()),
            root_certificate: Some(ROOT_CERTIFICATE.to_owned()),
            ..Default::default()
        });

        assert_eq!(
            tls.root_certificate.as_deref(),
            Some(ROOT_CERTIFICATE.trim())
        );
    }

    #[test]
    fn a_root_certificate_that_is_not_a_certificate_names_its_variable() {
        let message = rejection(CredentialSources {
            inline_url: Some(INLINE_URL.to_owned()),
            root_certificate: Some("not-a-certificate".to_owned()),
            ..Default::default()
        });

        assert_eq!(
            message,
            format!(
                "{DATABASE_ROOT_CERTIFICATE} is not a certificate: \
                 must be one or more PEM certificates"
            )
        );
    }

    /// What a local run would hit if it kept a certificate beside the `disable`
    /// that `make migrate` sets.
    #[test]
    fn a_root_certificate_is_refused_when_the_transport_is_disabled() {
        let message = rejection(CredentialSources {
            inline_url: Some(INLINE_URL.to_owned()),
            tls: Some("disable".to_owned()),
            root_certificate: Some(ROOT_CERTIFICATE.to_owned()),
            ..Default::default()
        });

        assert_eq!(message, unused_root_certificate_message());
    }

    /// The certificate comes from the configuration and the variable lowers the
    /// transport, so the certificate variable is not set at all. A message that
    /// blamed it would send the operator to an empty variable.
    #[test]
    fn a_configured_root_certificate_is_refused_when_the_variable_disables_the_transport() {
        let base = fs::read_to_string(LOCAL_CONFIG).expect("local config readable");
        let indented: String = ROOT_CERTIFICATE
            .lines()
            .map(|line| format!("    {line}\n"))
            .collect();
        let verified = TempFile::holding(&base.replace(
            "  tls: \"disable\"",
            &format!("  tls: \"verify-full\"\n  tls_root_certificate: |\n{indented}"),
        ));

        let message = rejection(CredentialSources {
            config_path: Some(verified.path.clone()),
            tls: Some("disable".to_owned()),
            ..Default::default()
        });

        assert_eq!(message, unused_root_certificate_message());
    }

    fn unused_root_certificate_message() -> String {
        format!(
            "the transport is `disable`, which consults no certificate, but a root \
             certificate is set. Check {DATABASE_TLS} and {DATABASE_ROOT_CERTIFICATE}."
        )
    }

    #[test]
    fn an_unrecognised_tls_mode_names_its_variable() {
        let message = rejection(CredentialSources {
            inline_url: Some(INLINE_URL.to_owned()),
            tls: Some("require".to_owned()),
            ..Default::default()
        });

        assert_eq!(
            message,
            format!("{DATABASE_TLS} is not a TLS mode: must be `disable` or `verify-full`")
        );
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
            ..Default::default()
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
        persistence::conn(&fixture.url, &DatabaseTlsConfig::DISABLED)
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

        let report = apply(
            &empty_database(&fixture).await,
            &DatabaseTlsConfig::DISABLED,
        )
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

        let report = apply(
            &SecretString::from(fixture.url.clone()),
            &DatabaseTlsConfig::DISABLED,
        )
        .await
        .expect("the repeat run succeeds");

        assert_eq!(applied(&report), Vec::<String>::new());
    }

    #[tokio::test]
    async fn a_run_writes_no_seed_data() {
        let fixture = test_support::database().await;
        let url = empty_database(&fixture).await;

        apply(&url, &DatabaseTlsConfig::DISABLED)
            .await
            .expect("migrations apply");

        let client = persistence::conn(url.expose_secret(), &DatabaseTlsConfig::DISABLED)
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

        let blocker = persistence::conn(&fixture.url, &DatabaseTlsConfig::DISABLED)
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
            apply(
                &SecretString::from(fixture.url.clone()),
                &DatabaseTlsConfig::DISABLED,
            ),
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
