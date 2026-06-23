use std::{collections::HashSet, path::PathBuf};

use secrecy::SecretString;
use serde::Deserialize;

use crate::{validate, validation::Validation};

use super::{helpers::socket_addr, ConfigFieldError, ServerConfig};
use super::{
    helpers::{
        gcs_credentials_mode, nonzero_u16, nonzero_u64, nonzero_usize, optional_url,
        parse_log_format, paseto_download_key, path_string, postgres_connection_string,
        public_api_base_url as validate_public_api_base_url, string_url, string_value,
        ConfigValidationExt,
    },
    Auth0Config, GcsObjectStorageConfig, HealthConfig, McpConfig, ObjectStorageConfig,
    ObservabilityConfig, PasetoConfig, PasetoDownloadConfig, PasetoDownloadKey, PubSubConfig,
    PubSubSubscriptionsConfig, ScannerConfig, UploadsConfig, WorkerConfig,
};

#[derive(Debug, Deserialize)]
pub(super) struct RawAppConfig {
    pub(super) server: RawServerConfig,
    pub(super) postgres: SecretString,
    pub(super) pubsub: RawPubSubConfig,
    pub(super) auth0: RawAuth0Config,
    pub(super) paseto: RawPasetoConfig,
    pub(super) object_storage: RawObjectStorageConfig,
    pub(super) scanner: RawScannerConfig,
    pub(super) uploads: RawUploadsConfig,
    pub(super) observability: RawObservabilityConfig,
    pub(super) worker: RawWorkerConfig,
    pub(super) mcp: RawMcpConfig,
    pub(super) health: RawHealthConfig,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawScannerConfig {
    clamd_address: String,
    connection_timeout_ms: u64,
    scan_timeout_ms: u64,
}

impl RawScannerConfig {
    pub(super) fn validate(self) -> Validation<ScannerConfig, ConfigFieldError> {
        validate! {
            clamd_address <- socket_addr(self.clamd_address).at("scanner.clamd_address"),
            connection_timeout_ms <- nonzero_u64(self.connection_timeout_ms)
                .at("scanner.connection_timeout_ms"),
            scan_timeout_ms <- nonzero_u64(self.scan_timeout_ms)
                .at("scanner.scan_timeout_ms"),
            => ScannerConfig {
                clamd_address,
                connection_timeout_ms,
                scan_timeout_ms,
            },
        }
    }
}

impl RawAppConfig {}

pub(super) fn validate_postgres_connection_string(
    value: SecretString,
) -> Validation<SecretString, ConfigFieldError> {
    postgres_connection_string(value).at("postgres")
}

#[derive(Debug, Deserialize)]
pub(super) struct RawAuth0Config {
    issuer: String,
    audience: String,
    jwks_url: String,
}

impl RawAuth0Config {
    pub(super) fn validate(self) -> Validation<Auth0Config, ConfigFieldError> {
        validate! {
            issuer <- string_url(self.issuer).at("auth0.issuer"),
            audience <- string_value(self.audience).at("auth0.audience"),
            jwks_url <- string_url(self.jwks_url).at("auth0.jwks_url"),
            => Auth0Config {
                issuer,
                audience,
                jwks_url,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPasetoConfig {
    download: RawPasetoDownloadConfig,
}

impl RawPasetoConfig {
    pub(super) fn validate(self) -> Validation<PasetoConfig, ConfigFieldError> {
        validate! {
            download <- self.download.validate(),
            => PasetoConfig { download },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPasetoDownloadConfig {
    active_key_id: String,
    keys: Vec<RawPasetoDownloadKey>,
}

impl RawPasetoDownloadConfig {
    pub(super) fn validate(self) -> Validation<PasetoDownloadConfig, ConfigFieldError> {
        validate! {
            active_key_id <- string_value(self.active_key_id).at("paseto.download.active_key_id"),
            keys <- validate_download_keys(self.keys),
            => PasetoDownloadConfig {
                active_key_id,
                keys,
            },
        }
        .and_then(validate_download_keyring)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPasetoDownloadKey {
    id: String,
    secret: SecretString,
}

impl RawPasetoDownloadKey {
    fn validate(self, index: usize) -> Validation<PasetoDownloadKey, ConfigFieldError> {
        let id_path = format!("paseto.download.keys[{index}].id");
        let secret_path = format!("paseto.download.keys[{index}].secret");

        validate! {
            id <- string_value(self.id).at(id_path),
            secret <- paseto_download_key(self.secret).at(secret_path),
            => PasetoDownloadKey { id, secret },
        }
    }
}

fn validate_download_keys(
    keys: Vec<RawPasetoDownloadKey>,
) -> Validation<Vec<PasetoDownloadKey>, ConfigFieldError> {
    let mut errors = Vec::new();
    let mut validated = Vec::with_capacity(keys.len());

    for (index, key) in keys.into_iter().enumerate() {
        match key.validate(index) {
            Validation::Valid(key) => validated.push(key),
            Validation::Invalid(mut key_errors) => errors.append(&mut key_errors),
        }
    }

    if validated.is_empty() {
        errors.push(ConfigFieldError::new(
            "paseto.download.keys",
            "must contain at least one key",
        ));
    }

    add_duplicate_id_errors(
        "paseto.download.keys",
        validated.iter().map(|key| key.id.as_str()),
        &mut errors,
    );

    if errors.is_empty() {
        return Validation::valid(validated);
    }

    Validation::invalid_many(errors)
}

fn validate_download_keyring(
    config: PasetoDownloadConfig,
) -> Validation<PasetoDownloadConfig, ConfigFieldError> {
    if config.keys.iter().any(|key| key.id == config.active_key_id) {
        return Validation::valid(config);
    }

    Validation::invalid(ConfigFieldError::new(
        "paseto.download.active_key_id",
        "must exist in paseto.download.keys",
    ))
}

fn add_duplicate_id_errors<'a>(
    path: &'static str,
    ids: impl IntoIterator<Item = &'a str>,
    errors: &mut Vec<ConfigFieldError>,
) {
    let mut seen = HashSet::new();
    let mut duplicates = HashSet::new();

    for id in ids {
        if !seen.insert(id) {
            duplicates.insert(id);
        }
    }

    if !duplicates.is_empty() {
        errors.push(ConfigFieldError::new(path, "key IDs must be unique"));
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawServerConfig {
    api_bind: String,
    worker_bind: String,
    mcp_bind: String,
    public_api_base_url: String,
}

impl RawServerConfig {
    pub(super) fn validate(self) -> Validation<ServerConfig, ConfigFieldError> {
        validate! {
            api_bind <- socket_addr(self.api_bind).at("server.api_bind"),
            worker_bind <- socket_addr(self.worker_bind).at("server.worker_bind"),
            mcp_bind <- socket_addr(self.mcp_bind).at("server.mcp_bind"),
            public_api_base_url <- validate_public_api_base_url(self.public_api_base_url)
                .at("server.public_api_base_url"),
            => ServerConfig {
                api_bind,
                worker_bind,
                mcp_bind,
                public_api_base_url,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPubSubConfig {
    project_id: String,
    subscriptions: RawPubSubSubscriptionsConfig,
}

impl RawPubSubConfig {
    pub(super) fn validate(self) -> Validation<PubSubConfig, ConfigFieldError> {
        validate! {
            project_id <- string_value(self.project_id).at("pubsub.project_id"),
            subscriptions <- self.subscriptions.validate(),
            => PubSubConfig {
                project_id,
                subscriptions,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPubSubSubscriptionsConfig {
    worker: String,
    worker_push_endpoint: String,
    worker_max_delivery_attempts: u16,
}

impl RawPubSubSubscriptionsConfig {
    pub(super) fn validate(self) -> Validation<PubSubSubscriptionsConfig, ConfigFieldError> {
        validate! {
            worker <- string_value(self.worker).at("pubsub.subscriptions.worker"),
            worker_push_endpoint <- string_url(self.worker_push_endpoint)
                .at("pubsub.subscriptions.worker_push_endpoint"),
            worker_max_delivery_attempts <- validate_worker_max_delivery_attempts(
                self.worker_max_delivery_attempts
            )
                .at("pubsub.subscriptions.worker_max_delivery_attempts"),
            => PubSubSubscriptionsConfig {
                worker,
                worker_push_endpoint,
                worker_max_delivery_attempts,
            },
        }
    }
}

fn validate_worker_max_delivery_attempts(value: u16) -> Result<u16, String> {
    if (5..=100).contains(&value) {
        Ok(value)
    } else {
        Err("must be between 5 and 100".into())
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case")]
pub(super) enum RawObjectStorageConfig {
    Filesystem {
        root: String,
    },
    Gcs {
        bucket: String,
        endpoint_override: Option<String>,
        credentials_mode: String,
        object_key_prefix: String,
    },
}

impl RawObjectStorageConfig {
    pub(super) fn validate(self) -> Validation<ObjectStorageConfig, ConfigFieldError> {
        match self {
            Self::Filesystem { root } => string_value(root)
                .at("object_storage.root")
                .map(PathBuf::from)
                .map(|root| ObjectStorageConfig::Filesystem { root }),
            Self::Gcs {
                bucket: raw_bucket,
                endpoint_override: raw_endpoint_override,
                credentials_mode: raw_credentials_mode,
                object_key_prefix: raw_object_key_prefix,
            } => validate! {
                bucket <- string_value(raw_bucket).at("object_storage.bucket"),
                endpoint_override <- optional_url(raw_endpoint_override)
                    .at("object_storage.endpoint_override"),
                credentials_mode <- gcs_credentials_mode(raw_credentials_mode)
                    .at("object_storage.credentials_mode"),
                object_key_prefix <- string_value(raw_object_key_prefix)
                    .at("object_storage.object_key_prefix"),
                => ObjectStorageConfig::Gcs(GcsObjectStorageConfig {
                    bucket,
                    endpoint_override,
                    credentials_mode,
                    object_key_prefix,
                }),
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawUploadsConfig {
    max_attachment_bytes: usize,
}

impl RawUploadsConfig {
    pub(super) fn validate(self) -> Validation<UploadsConfig, ConfigFieldError> {
        nonzero_usize(self.max_attachment_bytes)
            .at("uploads.max_attachment_bytes")
            .map(|max_attachment_bytes| UploadsConfig {
                max_attachment_bytes,
            })
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawObservabilityConfig {
    log_format: String,
    default_filter: String,
}

impl RawObservabilityConfig {
    pub(super) fn validate(self) -> Validation<ObservabilityConfig, ConfigFieldError> {
        validate! {
            log_format <- parse_log_format(self.log_format).at("observability.log_format"),
            default_filter <- string_value(self.default_filter).at("observability.default_filter"),
            => ObservabilityConfig {
                log_format,
                default_filter,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawWorkerConfig {
    concurrency: u16,
    retry_attempts: u16,
    shutdown_grace_seconds: u64,
}

impl RawWorkerConfig {
    pub(super) fn validate(self) -> Validation<WorkerConfig, ConfigFieldError> {
        validate! {
            concurrency <- nonzero_u16(self.concurrency).at("worker.concurrency"),
            retry_attempts <- Validation::valid(self.retry_attempts),
            shutdown_grace_seconds <- nonzero_u64(self.shutdown_grace_seconds)
                .at("worker.shutdown_grace_seconds"),
            => WorkerConfig {
                concurrency,
                retry_attempts,
                shutdown_grace_seconds,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawMcpConfig {
    shutdown_grace_seconds: u64,
}

impl RawMcpConfig {
    pub(super) fn validate(self) -> Validation<McpConfig, ConfigFieldError> {
        nonzero_u64(self.shutdown_grace_seconds)
            .at("mcp.shutdown_grace_seconds")
            .map(|shutdown_grace_seconds| McpConfig {
                shutdown_grace_seconds,
            })
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawHealthConfig {
    live_path: String,
    ready_path: String,
    dependency_timeout_ms: u64,
}

impl RawHealthConfig {
    pub(super) fn validate(self) -> Validation<HealthConfig, ConfigFieldError> {
        validate! {
            live_path <- path_string(self.live_path).at("health.live_path"),
            ready_path <- path_string(self.ready_path).at("health.ready_path"),
            dependency_timeout_ms <- nonzero_u64(self.dependency_timeout_ms)
                .at("health.dependency_timeout_ms"),
            => HealthConfig {
                live_path,
                ready_path,
                dependency_timeout_ms,
            },
        }
    }
}
