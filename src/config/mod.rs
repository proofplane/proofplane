use crate::{validate, validation::Validation};
use secrecy::SecretString;
use std::{
    env, fmt, fs, io,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use thiserror::Error;
use url::Url;

use raw::RawAppConfig;

mod helpers;
mod raw;
#[cfg(test)]
pub(crate) mod test_support;

/// Shared with the migration command, which validates a database URL, a TLS
/// mode, and a root certificate that never pass through an application
/// configuration file.
pub use helpers::{database_root_certificate, database_tls, postgres_connection_string};

pub const PROOFPLANE_CONFIG: &str = "PROOFPLANE_CONFIG";

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub pubsub: PubSubConfig,
    pub auth0: Auth0Config,
    pub paseto: PasetoConfig,
    pub object_storage: ObjectStorageConfig,
    pub scanner: ScannerConfig,
    pub uploads: UploadsConfig,
    pub observability: ObservabilityConfig,
    pub worker: WorkerConfig,
    pub mcp: McpConfig,
    pub health: HealthConfig,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub api_bind: SocketAddr,
    pub worker_bind: SocketAddr,
    pub mcp_bind: SocketAddr,
    pub public_api_base_url: Url,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: SecretString,
    pub tls: DatabaseTlsConfig,
    pub pool: DatabasePoolConfig,
}

/// The transport for a database connection.
///
/// The mode is separate from the certificate so that the mode alone can go into
/// a log line or an error message. A certificate is public, but it is long, and
/// neither a log nor an error is the place for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseTlsConfig {
    pub mode: DatabaseTls,
    /// One or more PEM certificates to trust in addition to the system
    /// certificate store. Supabase issues a root of its own, which no system
    /// store carries.
    pub root_certificate: Option<String>,
}

impl DatabaseTlsConfig {
    /// Plaintext, with no extra trust anchor. The local stack serves no
    /// certificate, so every fixture uses this.
    pub const DISABLED: Self = Self {
        mode: DatabaseTls::Disable,
        root_certificate: None,
    };
}

/// How a database connection is encrypted.
///
/// There is no mode that encrypts without verification. Such a mode would let a
/// release claim a guarantee that it does not hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseTls {
    /// Plaintext. The local stack serves no certificate.
    Disable,
    /// Encrypted, with the certificate chain and the hostname verified against
    /// the system certificate store.
    VerifyFull,
}

impl fmt::Display for DatabaseTls {
    /// The spelling the configuration file and the migration command accept.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Disable => "disable",
            Self::VerifyFull => "verify-full",
        };

        f.write_str(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatabasePoolConfig {
    pub api: usize,
    pub mcp: usize,
    pub worker: usize,
    pub dequeuer: usize,
    pub acquire_timeout_ms: u64,
    pub idle_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubSubConfig {
    pub project_id: String,
    pub subscriptions: PubSubSubscriptionsConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubSubSubscriptionsConfig {
    pub worker: String,
    pub worker_push_endpoint: Url,
    pub worker_max_delivery_attempts: u16,
}

#[derive(Debug, Clone)]
pub struct Auth0Config {
    pub issuer: Url,
    pub audience: String,
    pub jwks_url: Url,
    pub upstream_oauth: Auth0UpstreamOAuthConfig,
    pub auditor_portal: Auth0AuditorPortalConfig,
}

#[derive(Debug, Clone)]
pub struct Auth0UpstreamOAuthConfig {
    pub client_id: String,
    pub client_secret: SecretString,
    pub callback_path: String,
}

#[derive(Debug, Clone)]
pub struct Auth0AuditorPortalConfig {
    pub client_id: String,
    pub client_secret: SecretString,
    pub callback_path: String,
    pub callback_url: Url,
    pub connection: String,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
}

#[derive(Debug, Clone)]
pub struct PasetoConfig {
    pub download: PasetoDownloadConfig,
    pub upload_grant: PasetoUploadGrantConfig,
    pub mcp_oauth: PasetoMcpOAuthConfig,
}

#[derive(Debug, Clone)]
pub struct PasetoDownloadConfig {
    pub active_key_id: String,
    pub keys: Vec<PasetoDownloadKey>,
}

#[derive(Debug, Clone)]
pub struct PasetoDownloadKey {
    pub id: String,
    pub secret: SecretString,
}

#[derive(Debug, Clone)]
pub struct PasetoUploadGrantConfig {
    pub active_key_id: String,
    pub keys: Vec<PasetoUploadGrantKey>,
}

#[derive(Debug, Clone)]
pub struct PasetoUploadGrantKey {
    pub id: String,
    pub secret: SecretString,
}

#[derive(Debug, Clone)]
pub struct PasetoMcpOAuthConfig {
    pub active_key_id: String,
    pub keys: Vec<PasetoMcpOAuthKey>,
}

#[derive(Debug, Clone)]
pub struct PasetoMcpOAuthKey {
    pub id: String,
    pub secret: SecretString,
}

/// Where the two document object stores live.
///
/// Uploaded bytes land in the quarantine store and stay there until the worker
/// scans them. Only the worker moves a clean document into the evidence store.
/// One backend serves both targets, so the pair can never disagree about which
/// backend is in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectStorageConfig {
    Filesystem(FilesystemObjectStorageConfig),
    Gcs(GcsObjectStorageConfig),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemObjectStorageConfig {
    pub quarantine_root: PathBuf,
    pub evidence_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcsObjectStorageConfig {
    pub quarantine_bucket: String,
    pub evidence_bucket: String,
    /// Prepended to every physical object name in both buckets. It names the
    /// environment, not the role, so both stores share it.
    pub object_key_prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadsConfig {
    pub max_document_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannerConfig {
    pub clamd_address: SocketAddr,
    pub connection_timeout_ms: u64,
    pub scan_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservabilityConfig {
    pub log_format: LogFormat,
    pub default_filter: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Pretty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerConfig {
    pub concurrency: u16,
    pub retry_attempts: u16,
    pub shutdown_grace_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpConfig {
    pub shutdown_grace_seconds: u64,
    pub resource: Url,
    pub allowed_hosts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthConfig {
    pub live_path: String,
    pub ready_path: String,
    pub dependency_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{path}: {message}")]
pub struct ConfigFieldError {
    pub path: String,
    pub message: String,
}

impl ConfigFieldError {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("environment variable {0} is required")]
    MissingEnv(&'static str),
    #[error("failed to read config file {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to load config file {path}")]
    Load {
        path: PathBuf,
        #[source]
        source: config::ConfigError,
    },
    #[error("config validation failed: {0:?}")]
    Validation(Vec<ConfigFieldError>),
}

pub fn load_from_env() -> Result<AppConfig, ConfigError> {
    let path =
        env::var(PROOFPLANE_CONFIG).map_err(|_| ConfigError::MissingEnv(PROOFPLANE_CONFIG))?;

    load_from_path(path)
}

pub fn load_from_path(path: impl AsRef<Path>) -> Result<AppConfig, ConfigError> {
    let path = path.as_ref();
    let path_buf = path.to_path_buf();

    fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path_buf.clone(),
        source,
    })?;

    let raw = config::Config::builder()
        .add_source(config::File::from(path_buf.clone()).format(config::FileFormat::Yaml))
        .build()
        .map_err(|source| ConfigError::Load {
            path: path_buf.clone(),
            source,
        })?
        .try_deserialize::<RawAppConfig>()
        .map_err(|source| ConfigError::Load {
            path: path_buf.clone(),
            source,
        })?;

    validate_raw_config(raw)
        .into_result()
        .map_err(ConfigError::Validation)
}

fn validate_raw_config(raw: RawAppConfig) -> Validation<AppConfig, ConfigFieldError> {
    validate! {
        server <- raw.server.validate(),
        database <- raw.database.validate(),
        pubsub <- raw.pubsub.validate(),
        auth0 <- raw.auth0.validate(),
        paseto <- raw.paseto.validate(),
        object_storage <- raw.object_storage.validate(),
        scanner <- raw.scanner.validate(),
        uploads <- raw.uploads.validate(),
        observability <- raw.observability.validate(),
        worker <- raw.worker.validate(),
        mcp <- raw.mcp.validate(),
        health <- raw.health.validate(),
        => {
            let auth0 = auth0.resolve(&server.public_api_base_url);
            AppConfig {
                server,
                database,
                pubsub,
                auth0,
                paseto,
                object_storage,
                scanner,
                uploads,
                observability,
                worker,
                mcp,
                health,
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;
    use crate::config::test_support::ROOT_CERTIFICATE;
    use std::{
        fs,
        sync::{
            atomic::{AtomicU64, Ordering},
            Mutex,
        },
    };

    static ENV_LOCK: Mutex<()> = Mutex::new(());
    static TEMP_CONFIG_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn defines_config_environment_variable_name() {
        assert_eq!(PROOFPLANE_CONFIG, "PROOFPLANE_CONFIG");
    }

    #[test]
    fn local_config_loads_successfully() {
        let config = load_from_path("config/local.yaml").expect("local config loads");

        assert_eq!(config.server.api_bind.to_string(), "127.0.0.1:3000");
        assert_eq!(
            config.server.public_api_base_url.as_str(),
            "http://127.0.0.1:3000/"
        );
        assert_eq!(
            config.pubsub.subscriptions.worker_push_endpoint.as_str(),
            "http://host.docker.internal:3001/pubsub/messages"
        );
        assert_eq!(config.pubsub.subscriptions.worker_max_delivery_attempts, 5);
        assert_eq!(
            config.object_storage,
            ObjectStorageConfig::Filesystem(FilesystemObjectStorageConfig {
                quarantine_root: PathBuf::from(".local/storage/quarantine"),
                evidence_root: PathBuf::from(".local/storage/evidence"),
            })
        );
        assert_eq!(config.uploads.max_document_bytes, 25 * 1024 * 1024);
        assert_eq!(config.database.tls, DatabaseTlsConfig::DISABLED);
        assert_eq!(config.database.pool.api, 10);
        assert_eq!(config.database.pool.mcp, 10);
        assert_eq!(config.database.pool.worker, 6);
        assert_eq!(config.database.pool.dequeuer, 2);
        assert_eq!(config.database.pool.acquire_timeout_ms, 5_000);
        assert_eq!(config.database.pool.idle_timeout_ms, 300_000);
        assert_eq!(config.scanner.clamd_address.to_string(), "127.0.0.1:3310");
        assert_eq!(config.scanner.connection_timeout_ms, 1000);
        assert_eq!(config.scanner.scan_timeout_ms, 30000);
        assert_eq!(config.mcp.shutdown_grace_seconds, 30);
        assert!(
            config.mcp.allowed_hosts.is_empty(),
            "allowed_hosts defaults to empty (rmcp loopback-only) when omitted"
        );
        assert_eq!(config.paseto.download.active_key_id, "local-download-001");
        assert_eq!(config.paseto.download.keys.len(), 1);
        assert_eq!(
            config.paseto.upload_grant.active_key_id,
            "local-upload-grant-001"
        );
        assert_eq!(config.paseto.upload_grant.keys.len(), 1);
        assert_eq!(
            config.auth0.auditor_portal.callback_url.as_str(),
            "http://127.0.0.1:3000/auditor-access/auth0/callback"
        );
        assert_eq!(
            config.auth0.auditor_portal.authorization_endpoint.as_str(),
            "https://dev-ctgrkbdrbodz4azi.us.auth0.com/authorize"
        );
        assert_eq!(
            config.auth0.auditor_portal.token_endpoint.as_str(),
            "https://dev-ctgrkbdrbodz4azi.us.auth0.com/oauth/token"
        );
    }

    #[test]
    fn mcp_allowed_hosts_parse_when_present() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");
        let with_hosts = base.replace(
            "allowed_hosts: []",
            "allowed_hosts:\n    - \"mcp.example.ngrok.app\"\n    - \"127.0.0.1:3002\"",
        );
        assert_ne!(base, with_hosts, "allowed_hosts injection anchor matched");

        let path = write_temp_config(&with_hosts);
        let config = load_from_path(&path).expect("config with allowed_hosts loads");

        assert_eq!(
            config.mcp.allowed_hosts,
            vec![
                "mcp.example.ngrok.app".to_owned(),
                "127.0.0.1:3002".to_owned()
            ]
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn the_verifying_tls_mode_loads() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");
        let verified = base.replace("tls: \"disable\"", "tls: \"verify-full\"");
        assert_ne!(base, verified, "tls replacement anchor matched");

        let path = write_temp_config(&verified);
        let config = load_from_path(&path).expect("a verifying configuration loads");

        assert_eq!(config.database.tls.mode, DatabaseTls::VerifyFull);
        assert_eq!(config.database.tls.root_certificate, None);

        let _ = fs::remove_file(path);
    }

    /// The YAML block scalar an operator writes, indented under the key.
    fn with_root_certificate(base: &str, pem: &str) -> String {
        let indented: String = pem.lines().map(|line| format!("    {line}\n")).collect();

        base.replace(
            "  tls: \"disable\"",
            &format!("  tls: \"verify-full\"\n  tls_root_certificate: |\n{indented}"),
        )
    }

    #[test]
    fn a_root_certificate_loads_beside_the_verifying_mode() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");
        let verified = with_root_certificate(&base, ROOT_CERTIFICATE);
        assert_ne!(base, verified, "tls replacement anchor matched");

        let path = write_temp_config(&verified);
        let config = load_from_path(&path).expect("a configuration with a root certificate loads");

        assert_eq!(config.database.tls.mode, DatabaseTls::VerifyFull);
        assert_eq!(
            config.database.tls.root_certificate.as_deref(),
            Some(ROOT_CERTIFICATE.trim())
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_root_certificate_that_is_not_a_certificate_names_the_field() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");
        let broken = base.replace(
            "  tls: \"disable\"",
            "  tls: \"verify-full\"\n  tls_root_certificate: \"not-a-certificate\"",
        );
        assert_ne!(base, broken, "tls replacement anchor matched");

        let path = write_temp_config(&broken);
        let error =
            load_from_path(&path).expect_err("a value that is not a certificate is invalid");

        assert!(matches!(
            error,
            ConfigError::Validation(ref errors)
                if errors.iter().any(|error| error.path == "database.tls_root_certificate"
                    && error.message == "must be one or more PEM certificates")
        ));

        let _ = fs::remove_file(path);
    }

    /// A certificate no connection can consult is a configuration the operator
    /// did not mean to write, so it is refused rather than ignored.
    #[test]
    fn a_root_certificate_is_refused_when_the_transport_is_disabled() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");
        let indented: String = ROOT_CERTIFICATE
            .lines()
            .map(|line| format!("    {line}\n"))
            .collect();
        let mismatched = base.replace(
            "  tls: \"disable\"",
            &format!("  tls: \"disable\"\n  tls_root_certificate: |\n{indented}"),
        );
        assert_ne!(base, mismatched, "tls replacement anchor matched");

        let path = write_temp_config(&mismatched);
        let error = load_from_path(&path).expect_err("a certificate without TLS is invalid");

        assert!(matches!(
            error,
            ConfigError::Validation(ref errors)
                if errors.iter().any(|error| error.path == "database.tls_root_certificate"
                    && error.message == "must not be set when database.tls is `disable`")
        ));

        let _ = fs::remove_file(path);
    }

    /// The operator sees this message and nothing else, so it has to say which
    /// field is absent.
    #[test]
    fn a_missing_tls_mode_names_the_field() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");
        let without = base.replace("  tls: \"disable\"\n", "");
        assert_ne!(base, without, "tls removal anchor matched");

        let path = write_temp_config(&without);
        let error = load_from_path(&path).expect_err("a configuration without a mode is invalid");

        let reason = format!("{:#}", anyhow::Error::new(error));
        assert!(
            reason.contains("database.tls"),
            "expected the missing field to be named, got: {reason}"
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn gcs_configuration_names_separate_quarantine_and_evidence_buckets() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");
        let gcs = base.replace(
            "object_storage:\n  backend: \"filesystem\"\n  quarantine_root: \".local/storage/quarantine\"\n  evidence_root: \".local/storage/evidence\"",
            "object_storage:\n  backend: \"gcs\"\n  quarantine_bucket: \"proofplane-test-quarantine\"\n  evidence_bucket: \"proofplane-test-evidence\"\n  object_key_prefix: \"environments/test\"",
        );
        assert_ne!(base, gcs, "object storage replacement anchor matched");

        let path = write_temp_config(&gcs);
        let config = load_from_path(&path).expect("GCS config loads");

        assert_eq!(
            config.object_storage,
            ObjectStorageConfig::Gcs(GcsObjectStorageConfig {
                quarantine_bucket: "proofplane-test-quarantine".to_owned(),
                evidence_bucket: "proofplane-test-evidence".to_owned(),
                object_key_prefix: "environments/test".to_owned(),
            })
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn gcs_object_key_prefix_must_be_a_canonical_relative_path() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");

        for prefix in [
            "/proofplane",
            "proofplane/",
            "proofplane//test",
            "proofplane/../test",
        ] {
            let gcs = base.replace(
                "object_storage:\n  backend: \"filesystem\"\n  quarantine_root: \".local/storage/quarantine\"\n  evidence_root: \".local/storage/evidence\"",
                &format!(
                    "object_storage:\n  backend: \"gcs\"\n  quarantine_bucket: \"proofplane-test-quarantine\"\n  evidence_bucket: \"proofplane-test-evidence\"\n  object_key_prefix: \"{prefix}\""
                ),
            );
            let path = write_temp_config(&gcs);
            let error = load_from_path(&path).expect_err("non-canonical prefix is rejected");

            assert!(matches!(
                error,
                ConfigError::Validation(ref errors)
                    if errors.iter().any(|error| error.path == "object_storage.object_key_prefix")
            ));

            let _ = fs::remove_file(path);
        }
    }

    /// One target for both stores would put unscanned bytes where the evidence
    /// lives, and would expose the evidence to the quarantine expiry rule.
    #[test]
    fn object_storage_rejects_one_target_used_for_both_stores() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");
        let cases = [
            (
                "object_storage:\n  backend: \"filesystem\"\n  quarantine_root: \".local/storage\"\n  evidence_root: \".local/storage\"",
                "object_storage.evidence_root",
            ),
            (
                "object_storage:\n  backend: \"gcs\"\n  quarantine_bucket: \"proofplane-test\"\n  evidence_bucket: \"proofplane-test\"\n  object_key_prefix: \"environments/test\"",
                "object_storage.evidence_bucket",
            ),
        ];

        for (object_storage, expected_path) in cases {
            let shared = base.replace(
                "object_storage:\n  backend: \"filesystem\"\n  quarantine_root: \".local/storage/quarantine\"\n  evidence_root: \".local/storage/evidence\"",
                object_storage,
            );
            assert_ne!(base, shared, "object storage replacement anchor matched");

            let path = write_temp_config(&shared);
            let error = load_from_path(&path).expect_err("one shared target is rejected");

            assert!(
                matches!(
                    error,
                    ConfigError::Validation(ref errors)
                        if errors.iter().any(|error| error.path == expected_path)
                ),
                "expected {expected_path} to be rejected"
            );

            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn missing_env_var_returns_error() {
        let _lock = ENV_LOCK.lock().expect("env lock is available");
        let previous = env::var(PROOFPLANE_CONFIG).ok();

        env::remove_var(PROOFPLANE_CONFIG);

        let error = load_from_env().expect_err("env var is missing");

        if let Some(previous) = previous {
            env::set_var(PROOFPLANE_CONFIG, previous);
        }

        assert!(matches!(error, ConfigError::MissingEnv(PROOFPLANE_CONFIG)));
    }

    #[test]
    fn missing_file_returns_read_error() {
        let error = load_from_path("config/does-not-exist.yaml").expect_err("file is missing");

        assert!(matches!(error, ConfigError::Read { .. }));
    }

    #[test]
    fn malformed_yaml_returns_load_error() {
        let path = write_temp_config("malformed: [");
        let error = load_from_path(&path).expect_err("yaml is malformed");

        assert!(matches!(error, ConfigError::Load { .. }));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_required_fields_fail_during_deserialization() {
        let path = write_temp_config(
            r#"
environment: ""
server: {}
database: {}
pubsub: {}
object_storage: {}
scanner: {}
observability: {}
worker: {}
health: {}
"#,
        );

        let error = load_from_path(&path).expect_err("config is invalid");

        assert!(matches!(error, ConfigError::Load { .. }));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn invalid_values_are_rejected() {
        let path = write_temp_config(
            r#"
environment: local
server:
  api_bind: "not-a-socket"
  worker_bind: "127.0.0.1:3001"
  mcp_bind: "127.0.0.1:3002"
  public_api_base_url: "http://example.com/api"
database:
  url: ""
  tls: "require"
  pool:
    api: 0
    mcp: 0
    worker: 0
    dequeuer: 0
    acquire_timeout_ms: 0
    idle_timeout_ms: 0
pubsub:
  project_id: "proofplane-local"
  subscriptions:
    worker: "proofplane-worker"
    worker_push_endpoint: "not-a-url"
    worker_max_delivery_attempts: 4
auth0:
  issuer: ""
  audience: ""
  jwks_url: "not-a-url"
  upstream_oauth:
    client_id: ""
    client_secret: ""
    callback_path: "callback"
  auditor_portal:
    client_id: ""
    client_secret: ""
    callback_path: "../callback"
    connection: "sms"
paseto:
  download:
    active_key_id: ""
    keys:
      - id: ""
        secret: "not-a-paserk"
  upload_grant:
    active_key_id: ""
    keys:
      - id: ""
        secret: "not-a-paserk"
  mcp_oauth:
    active_key_id: ""
    keys:
      - id: ""
        secret: "not-a-paserk"
object_storage:
  backend: "gcs"
  quarantine_bucket: "proofplane"
  evidence_bucket: "proofplane"
  object_key_prefix: "../evidence"
scanner:
  clamd_address: "not-a-socket"
  connection_timeout_ms: 0
  scan_timeout_ms: 0
uploads:
  max_document_bytes: 0
observability:
  log_format: "xml"
  default_filter: "info"
worker:
  concurrency: 0
  retry_attempts: 0
  shutdown_grace_seconds: 0
mcp:
  shutdown_grace_seconds: 0
  resource: "not-a-url"
health:
  live_path: "livez"
  ready_path: "/readyz"
  dependency_timeout_ms: 0
"#,
        );

        let error = load_from_path(&path).expect_err("config is invalid");

        match error {
            ConfigError::Validation(errors) => {
                let paths = errors
                    .iter()
                    .map(|error| error.path.as_str())
                    .collect::<Vec<_>>();

                assert!(paths.contains(&"server.api_bind"));
                assert!(paths.contains(&"server.public_api_base_url"));
                // Every bad database field is reported in one pass rather than
                // the validator stopping at the first.
                assert!(paths.contains(&"database.url"));
                assert!(paths.contains(&"database.tls"));
                assert!(paths.contains(&"database.pool.api"));
                assert!(paths.contains(&"database.pool.mcp"));
                assert!(paths.contains(&"database.pool.worker"));
                assert!(paths.contains(&"database.pool.dequeuer"));
                assert!(paths.contains(&"database.pool.acquire_timeout_ms"));
                assert!(paths.contains(&"database.pool.idle_timeout_ms"));
                assert!(paths.contains(&"pubsub.subscriptions.worker_push_endpoint"));
                assert!(paths.contains(&"pubsub.subscriptions.worker_max_delivery_attempts"));
                assert!(paths.contains(&"auth0.issuer"));
                assert!(paths.contains(&"auth0.audience"));
                assert!(paths.contains(&"auth0.jwks_url"));
                assert!(paths.contains(&"auth0.upstream_oauth.client_id"));
                assert!(paths.contains(&"auth0.upstream_oauth.client_secret"));
                assert!(paths.contains(&"auth0.upstream_oauth.callback_path"));
                assert!(paths.contains(&"auth0.auditor_portal.client_id"));
                assert!(paths.contains(&"auth0.auditor_portal.client_secret"));
                assert!(paths.contains(&"auth0.auditor_portal.callback_path"));
                assert!(paths.contains(&"auth0.auditor_portal.connection"));
                assert!(paths.contains(&"paseto.download.active_key_id"));
                assert!(paths.contains(&"paseto.download.keys[0].id"));
                assert!(paths.contains(&"paseto.download.keys[0].secret"));
                assert!(paths.contains(&"paseto.upload_grant.active_key_id"));
                assert!(paths.contains(&"paseto.upload_grant.keys[0].id"));
                assert!(paths.contains(&"paseto.upload_grant.keys[0].secret"));
                assert!(paths.contains(&"paseto.mcp_oauth.active_key_id"));
                assert!(paths.contains(&"paseto.mcp_oauth.keys[0].id"));
                assert!(paths.contains(&"paseto.mcp_oauth.keys[0].secret"));
                assert!(paths.contains(&"object_storage.object_key_prefix"));
                assert!(paths.contains(&"scanner.clamd_address"));
                assert!(paths.contains(&"scanner.connection_timeout_ms"));
                assert!(paths.contains(&"scanner.scan_timeout_ms"));
                assert!(paths.contains(&"uploads.max_document_bytes"));
                assert!(paths.contains(&"observability.log_format"));
                assert!(paths.contains(&"worker.concurrency"));
                assert!(paths.contains(&"worker.shutdown_grace_seconds"));
                assert!(paths.contains(&"mcp.shutdown_grace_seconds"));
                assert!(paths.contains(&"mcp.resource"));
                assert!(paths.contains(&"health.live_path"));
                assert!(paths.contains(&"health.dependency_timeout_ms"));
            }
            error => panic!("unexpected error: {error:?}"),
        }

        let _ = fs::remove_file(path);
    }

    #[test]
    fn config_field_error_display_includes_path_and_message() {
        let error = ConfigFieldError::new("server.api_bind", "must be a socket address");

        assert_eq!(
            error.to_string(),
            "server.api_bind: must be a socket address"
        );
    }

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let postgres = SecretString::from(
            "postgres://proofplane:unique-secret-password@127.0.0.1:5432/proofplane",
        );
        let debug = format!("{:?}", postgres);

        assert!(!debug.contains(postgres.expose_secret()));
        assert!(debug.contains("Secret"));
    }

    #[test]
    fn paseto_secrets_are_redacted_in_debug_output() {
        let config = load_from_path("config/local.yaml").expect("local config loads");
        let debug = format!("{:?}", config.paseto);

        assert!(!debug.contains(config.paseto.download.keys[0].secret.expose_secret()));
        assert!(!debug.contains(config.paseto.upload_grant.keys[0].secret.expose_secret()));
        assert!(debug.contains("Secret"));
    }

    #[test]
    fn auditor_auth0_secret_is_redacted() {
        let config = load_from_path("config/local.yaml").expect("local config loads");
        let debug = format!("{:?}", config.auth0.auditor_portal);

        assert!(!debug.contains(config.auth0.auditor_portal.client_secret.expose_secret()));
        assert!(debug.contains("Secret"));
    }

    #[test]
    fn auditor_auth0_validation_errors_do_not_expose_the_secret() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");
        let unique_secret = "auditor-secret-that-must-not-appear";
        let modified = base
            .replace(
                "local-proofplane-auditor-client-secret-change-me",
                unique_secret,
            )
            .replace(
                "/auditor-access/auth0/callback",
                "/auditor-access/../oauth/callback",
            );
        let path = write_temp_config(&modified);

        let error = load_from_path(&path).expect_err("unsafe callback is invalid");
        assert!(!format!("{error:?}").contains(unique_secret));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn auditor_auth0_config_reports_missing_fields_individually() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");

        for (field, expected_path) in [
            (
                "    client_id: \"local-proofplane-auditor-client\"\n",
                "auth0.auditor_portal.client_id",
            ),
            (
                "    client_secret: \"local-proofplane-auditor-client-secret-change-me\"\n",
                "auth0.auditor_portal.client_secret",
            ),
            (
                "    callback_path: \"/auditor-access/auth0/callback\"\n",
                "auth0.auditor_portal.callback_path",
            ),
            (
                "    connection: \"email\"\n",
                "auth0.auditor_portal.connection",
            ),
        ] {
            let modified = base.replacen(field, "", 1);
            assert_ne!(base, modified, "auditor config field anchor matched");
            let path = write_temp_config(&modified);
            let error = load_from_path(&path).expect_err("missing auditor field is invalid");
            assert_validation_paths(error, &[expected_path]);
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn auditor_callback_rejects_malformed_or_escaping_paths() {
        let base = fs::read_to_string("config/local.yaml").expect("local config readable");

        for callback in [
            "",
            "/oauth/auth0/callback",
            "//evil.example/callback",
            "/auditor-access/../oauth/callback",
            "/auditor-access/%2e%2e/oauth/callback",
            "/auditor-access/auth0/callback?next=/oauth",
            "/auditor-access/auth0/callback#fragment",
            "/auditor-access/auth0\\callback",
        ] {
            let yaml_callback = callback.replace('\\', "\\\\");
            let modified = base.replace(
                "    callback_path: \"/auditor-access/auth0/callback\"",
                &format!("    callback_path: \"{yaml_callback}\""),
            );
            assert_ne!(base, modified, "auditor callback anchor matched");
            let path = write_temp_config(&modified);
            let error = load_from_path(&path).expect_err("unsafe auditor callback is invalid");
            assert_validation_paths(error, &["auth0.auditor_portal.callback_path"]);
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn paseto_keyring_validation_rejects_duplicate_ids() {
        let path = write_temp_config(&local_config_with_paseto(
            r#"
paseto:
  download:
    active_key_id: "duplicate"
    keys:
      - id: "duplicate"
        secret: "k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs"
      - id: "duplicate"
        secret: "k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs"
  upload_grant:
    active_key_id: "local-upload-grant-001"
    keys:
      - id: "local-upload-grant-001"
        secret: "k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY"
"#,
        ));

        let error = load_from_path(&path).expect_err("config is invalid");

        assert_validation_paths(error, &["paseto.download.keys"]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn paseto_keyring_validation_rejects_missing_active_ids() {
        let path = write_temp_config(&local_config_with_paseto(
            r#"
paseto:
  download:
    active_key_id: "missing-download"
    keys:
      - id: "local-download-001"
        secret: "k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs"
  upload_grant:
    active_key_id: "local-upload-grant-001"
    keys:
      - id: "local-upload-grant-001"
        secret: "k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY"
"#,
        ));

        let error = load_from_path(&path).expect_err("config is invalid");

        assert_validation_paths(error, &["paseto.download.active_key_id"]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn upload_grant_keyring_validation_rejects_duplicate_ids() {
        let path = write_temp_config(&local_config_with_paseto(
            r#"
paseto:
  download:
    active_key_id: "local-download-001"
    keys:
      - id: "local-download-001"
        secret: "k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs"
  upload_grant:
    active_key_id: "duplicate"
    keys:
      - id: "duplicate"
        secret: "k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY"
      - id: "duplicate"
        secret: "k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY"
"#,
        ));

        let error = load_from_path(&path).expect_err("config is invalid");

        assert_validation_paths(error, &["paseto.upload_grant.keys"]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn upload_grant_keyring_validation_rejects_missing_active_ids() {
        let path = write_temp_config(&local_config_with_paseto(
            r#"
paseto:
  download:
    active_key_id: "local-download-001"
    keys:
      - id: "local-download-001"
        secret: "k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs"
  upload_grant:
    active_key_id: "missing-upload-grant"
    keys:
      - id: "local-upload-grant-001"
        secret: "k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY"
"#,
        ));

        let error = load_from_path(&path).expect_err("config is invalid");

        assert_validation_paths(error, &["paseto.upload_grant.active_key_id"]);

        let _ = fs::remove_file(path);
    }

    fn assert_validation_paths(error: ConfigError, expected_paths: &[&str]) {
        let ConfigError::Validation(errors) = error else {
            panic!("unexpected error: {error:?}");
        };
        let paths = errors
            .iter()
            .map(|error| error.path.as_str())
            .collect::<Vec<_>>();

        for expected_path in expected_paths {
            assert!(
                paths.contains(expected_path),
                "missing {expected_path}; got {paths:?}"
            );
        }
    }

    fn local_config_with_paseto(paseto: &str) -> String {
        let local = fs::read_to_string("config/local.yaml").expect("local config reads");
        let before_paseto = local
            .split_once("\npaseto:\n")
            .expect("local config has paseto")
            .0;
        let after_paseto = local
            .split_once("\nobject_storage:\n")
            .expect("local config has object_storage")
            .1;

        let paseto = if paseto.contains("mcp_oauth:") {
            paseto.to_owned()
        } else {
            format!(
                "{paseto}  mcp_oauth:\n    active_key_id: \"local-mcp-oauth-001\"\n    keys:\n      - id: \"local-mcp-oauth-001\"\n        secret: \"k4.local.BMyQa9GmLofWmmvtYCedLfePwmuJsMgNn96nW1PtMp0\"\n"
            )
        };

        format!("{before_paseto}\n{paseto}\nobject_storage:\n{after_paseto}")
    }

    fn write_temp_config(contents: &str) -> PathBuf {
        let suffix = TEMP_CONFIG_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "proofplane-config-test-{}-{suffix}.yaml",
            std::process::id()
        ));

        fs::write(&path, contents).expect("temp config is written");

        path
    }
}
