use crate::{validate, validation::Validation};
use secrecy::SecretString;
use std::{
    env, fs, io,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use thiserror::Error;
use url::Url;

use raw::RawAppConfig;

mod helpers;
mod raw;

pub const PROOFPLANE_CONFIG: &str = "PROOFPLANE_CONFIG";

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub postgres: SecretString,
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
    pub mcp: Auth0McpConfig,
}

#[derive(Debug, Clone)]
pub struct Auth0McpConfig {
    pub resource: Url,
    pub allowed_client_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PasetoConfig {
    pub download: PasetoDownloadConfig,
    pub upload_grant: PasetoUploadGrantConfig,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectStorageConfig {
    Filesystem { root: PathBuf },
    Gcs(GcsObjectStorageConfig),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcsObjectStorageConfig {
    pub bucket: String,
    pub endpoint_override: Option<Url>,
    pub credentials_mode: GcsCredentialsMode,
    pub object_key_prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadsConfig {
    pub max_attachment_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannerConfig {
    pub clamd_address: SocketAddr,
    pub connection_timeout_ms: u64,
    pub scan_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GcsCredentialsMode {
    ApplicationDefault,
    Anonymous,
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
    #[error("failed to read config file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to load config file {path}: {source}")]
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
        postgres <- raw::validate_postgres_connection_string(raw.postgres),
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
        => AppConfig {
            server,
            postgres,
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
        },
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;
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
        assert!(matches!(
            config.object_storage,
            ObjectStorageConfig::Filesystem { .. }
        ));
        assert_eq!(config.uploads.max_attachment_bytes, 25 * 1024 * 1024);
        assert_eq!(config.scanner.clamd_address.to_string(), "127.0.0.1:3310");
        assert_eq!(config.scanner.connection_timeout_ms, 1000);
        assert_eq!(config.scanner.scan_timeout_ms, 30000);
        assert_eq!(config.mcp.shutdown_grace_seconds, 30);
        assert_eq!(config.paseto.download.active_key_id, "local-download-001");
        assert_eq!(config.paseto.download.keys.len(), 1);
        assert_eq!(
            config.paseto.upload_grant.active_key_id,
            "local-upload-grant-001"
        );
        assert_eq!(config.paseto.upload_grant.keys.len(), 1);
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
postgres: {}
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
postgres: ""
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
  mcp:
    resource: "not-a-url"
    allowed_client_ids: []
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
object_storage:
  backend: "gcs"
  bucket: "proofplane"
  endpoint_override: "not-a-url"
  credentials_mode: "unknown"
  object_key_prefix: "evidence"
scanner:
  clamd_address: "not-a-socket"
  connection_timeout_ms: 0
  scan_timeout_ms: 0
uploads:
  max_attachment_bytes: 0
observability:
  log_format: "xml"
  default_filter: "info"
worker:
  concurrency: 0
  retry_attempts: 0
  shutdown_grace_seconds: 0
mcp:
  shutdown_grace_seconds: 0
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
                assert!(paths.contains(&"postgres"));
                assert!(paths.contains(&"pubsub.subscriptions.worker_push_endpoint"));
                assert!(paths.contains(&"pubsub.subscriptions.worker_max_delivery_attempts"));
                assert!(paths.contains(&"auth0.issuer"));
                assert!(paths.contains(&"auth0.audience"));
                assert!(paths.contains(&"auth0.jwks_url"));
                assert!(paths.contains(&"auth0.mcp.resource"));
                assert!(paths.contains(&"auth0.mcp.allowed_client_ids"));
                assert!(paths.contains(&"paseto.download.active_key_id"));
                assert!(paths.contains(&"paseto.download.keys[0].id"));
                assert!(paths.contains(&"paseto.download.keys[0].secret"));
                assert!(paths.contains(&"paseto.upload_grant.active_key_id"));
                assert!(paths.contains(&"paseto.upload_grant.keys[0].id"));
                assert!(paths.contains(&"paseto.upload_grant.keys[0].secret"));
                assert!(paths.contains(&"object_storage.endpoint_override"));
                assert!(paths.contains(&"object_storage.credentials_mode"));
                assert!(paths.contains(&"scanner.clamd_address"));
                assert!(paths.contains(&"scanner.connection_timeout_ms"));
                assert!(paths.contains(&"scanner.scan_timeout_ms"));
                assert!(paths.contains(&"uploads.max_attachment_bytes"));
                assert!(paths.contains(&"observability.log_format"));
                assert!(paths.contains(&"worker.concurrency"));
                assert!(paths.contains(&"worker.shutdown_grace_seconds"));
                assert!(paths.contains(&"mcp.shutdown_grace_seconds"));
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
