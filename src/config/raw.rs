use std::path::PathBuf;

use secrecy::SecretString;
use serde::Deserialize;

use crate::{validate, validation::Validation};

use super::{helpers::socket_addr, ConfigFieldError, ServerConfig};
use super::{
    helpers::{
        gcs_credentials_mode, host_port, nonzero_u16, nonzero_u64, optional_url, parse_log_format,
        path_string, postgres_connection_string, secret_value, string_url, string_value,
        ConfigValidationExt,
    },
    AuthConfig, GcsObjectStorageConfig, HealthConfig, ObjectStorageConfig, ObservabilityConfig,
    PubSubConfig, PubSubSubscriptionsConfig, PubSubTopicsConfig, SpiceDbConfig, WorkerConfig,
};

#[derive(Debug, Deserialize)]
pub(super) struct RawAppConfig {
    pub(super) server: RawServerConfig,
    pub(super) postgres: SecretString,
    pub(super) pubsub: RawPubSubConfig,
    pub(super) spicedb: RawSpiceDbConfig,
    pub(super) object_storage: RawObjectStorageConfig,
    pub(super) observability: RawObservabilityConfig,
    pub(super) auth: RawAuthConfig,
    pub(super) worker: RawWorkerConfig,
    pub(super) health: RawHealthConfig,
}

impl RawAppConfig {}

pub(super) fn validate_postgres_connection_string(
    value: SecretString,
) -> Validation<SecretString, ConfigFieldError> {
    postgres_connection_string(value).at("postgres")
}

#[derive(Debug, Deserialize)]
pub(super) struct RawSpiceDbConfig {
    endpoint: String,
    preshared_key: SecretString,
    schema_path: String,
}

impl RawSpiceDbConfig {
    pub(super) fn validate(self) -> Validation<SpiceDbConfig, ConfigFieldError> {
        validate! {
            endpoint <- string_url(self.endpoint).at("spicedb.endpoint"),
            preshared_key <- secret_value(self.preshared_key).at("spicedb.preshared_key"),
            schema_path <- string_value(self.schema_path).at("spicedb.schema_path"),
            => SpiceDbConfig {
                endpoint,
                preshared_key,
                schema_path: PathBuf::from(schema_path),
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawServerConfig {
    api_bind: String,
    worker_bind: String,
    mcp_bind: String,
}

impl RawServerConfig {
    pub(super) fn validate(self) -> Validation<ServerConfig, ConfigFieldError> {
        validate! {
            api_bind <- socket_addr(self.api_bind).at("server.api_bind"),
            worker_bind <- socket_addr(self.worker_bind).at("server.worker_bind"),
            mcp_bind <- socket_addr(self.mcp_bind).at("server.mcp_bind"),
            => ServerConfig {
                api_bind,
                worker_bind,
                mcp_bind,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPubSubConfig {
    project_id: String,
    emulator_host: String,
    topics: RawPubSubTopicsConfig,
    subscriptions: RawPubSubSubscriptionsConfig,
}

impl RawPubSubConfig {
    pub(super) fn validate(self) -> Validation<PubSubConfig, ConfigFieldError> {
        validate! {
            project_id <- string_value(self.project_id).at("pubsub.project_id"),
            emulator_host <- host_port(self.emulator_host).at("pubsub.emulator_host"),
            topics <- self.topics.validate(),
            subscriptions <- self.subscriptions.validate(),
            => PubSubConfig {
                project_id,
                emulator_host,
                topics,
                subscriptions,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPubSubTopicsConfig {
    outbox: String,
    dead_letter: String,
}

impl RawPubSubTopicsConfig {
    pub(super) fn validate(self) -> Validation<PubSubTopicsConfig, ConfigFieldError> {
        validate! {
            outbox <- string_value(self.outbox).at("pubsub.topics.outbox"),
            dead_letter <- string_value(self.dead_letter).at("pubsub.topics.dead_letter"),
            => PubSubTopicsConfig {
                outbox,
                dead_letter,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPubSubSubscriptionsConfig {
    worker: String,
}

impl RawPubSubSubscriptionsConfig {
    pub(super) fn validate(self) -> Validation<PubSubSubscriptionsConfig, ConfigFieldError> {
        validate! {
            worker <- string_value(self.worker).at("pubsub.subscriptions.worker"),
            => PubSubSubscriptionsConfig {
                worker,
            },
        }
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
pub(super) struct RawAuthConfig {
    api_key_header: String,
    credential_hash_pepper: SecretString,
}

impl RawAuthConfig {
    pub(super) fn validate(self) -> Validation<AuthConfig, ConfigFieldError> {
        validate! {
            api_key_header <- string_value(self.api_key_header).at("auth.api_key_header"),
            credential_hash_pepper <- secret_value(self.credential_hash_pepper)
                .at("auth.credential_hash_pepper"),
            => AuthConfig {
                api_key_header,
                credential_hash_pepper,
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
