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
    pub spicedb: SpiceDbConfig,
    pub object_storage: ObjectStorageConfig,
    pub uploads: UploadsConfig,
    pub observability: ObservabilityConfig,
    pub worker: WorkerConfig,
    pub health: HealthConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub api_bind: SocketAddr,
    pub worker_bind: SocketAddr,
    pub mcp_bind: SocketAddr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubSubConfig {
    pub project_id: String,
    pub emulator_host: HostPort,
    pub subscriptions: PubSubSubscriptionsConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubSubSubscriptionsConfig {
    pub worker: String,
}

#[derive(Debug, Clone)]
pub struct SpiceDbConfig {
    pub endpoint: Url,
    pub preshared_key: SecretString,
    pub schema_path: PathBuf,
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
pub struct HealthConfig {
    pub live_path: String,
    pub ready_path: String,
    pub dependency_timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPort {
    pub host: String,
    pub port: u16,
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
        spicedb <- raw.spicedb.validate(),
        object_storage <- raw.object_storage.validate(),
        uploads <- raw.uploads.validate(),
        observability <- raw.observability.validate(),
        worker <- raw.worker.validate(),
        health <- raw.health.validate(),
        => AppConfig {
            server,
            postgres,
            pubsub,
            spicedb,
            object_storage,
            uploads,
            observability,
            worker,
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
            config.spicedb.schema_path,
            PathBuf::from("authz/spicedb/proofplane.zed")
        );
        assert!(matches!(
            config.object_storage,
            ObjectStorageConfig::Filesystem { .. }
        ));
        assert_eq!(config.uploads.max_attachment_bytes, 25 * 1024 * 1024);
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
spicedb: {}
object_storage: {}
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
postgres: ""
pubsub:
  project_id: "proofplane-local"
  emulator_host: "127.0.0.1:0"
  subscriptions:
    worker: "proofplane-worker"
spicedb:
  endpoint: ""
  preshared_key: ""
  schema_path: ""
object_storage:
  backend: "gcs"
  bucket: "proofplane"
  endpoint_override: "not-a-url"
  credentials_mode: "unknown"
  object_key_prefix: "evidence"
uploads:
  max_attachment_bytes: 0
observability:
  log_format: "xml"
  default_filter: "info"
worker:
  concurrency: 0
  retry_attempts: 0
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
                assert!(paths.contains(&"postgres"));
                assert!(paths.contains(&"pubsub.emulator_host"));
                assert!(paths.contains(&"spicedb.endpoint"));
                assert!(paths.contains(&"spicedb.preshared_key"));
                assert!(paths.contains(&"spicedb.schema_path"));
                assert!(paths.contains(&"object_storage.endpoint_override"));
                assert!(paths.contains(&"object_storage.credentials_mode"));
                assert!(paths.contains(&"uploads.max_attachment_bytes"));
                assert!(paths.contains(&"observability.log_format"));
                assert!(paths.contains(&"worker.concurrency"));
                assert!(paths.contains(&"worker.shutdown_grace_seconds"));
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
    fn spicedb_preshared_key_is_redacted_in_debug_output() {
        let config = load_from_path("config/local.yaml").expect("local config loads");
        let debug = format!("{:?}", config.spicedb.preshared_key);

        assert!(!debug.contains(config.spicedb.preshared_key.expose_secret()));
        assert!(debug.contains("Secret"));
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
