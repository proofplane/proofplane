use std::{collections::HashSet, path::PathBuf};

use secrecy::SecretString;
use serde::Deserialize;

use crate::{validate, validation::Validation};

use super::{helpers::socket_addr, ConfigFieldError, ServerConfig};
use super::{
    helpers::{
        auditor_callback_path, auditor_connection, database_root_certificate, database_tls,
        nonzero_u16, nonzero_u64, nonzero_usize, object_key_prefix as validate_object_key_prefix,
        parse_log_format, paseto_download_key, path_string, postgres_connection_string,
        public_api_base_url as validate_public_api_base_url, secret_value, string_url,
        string_value, ConfigValidationExt,
    },
    Auth0AuditorPortalConfig, Auth0Config, Auth0UpstreamOAuthConfig, DatabaseConfig,
    DatabasePoolConfig, DatabaseTlsConfig, FilesystemObjectStorageConfig, GcsObjectStorageConfig,
    HealthConfig, MailAdapterConfig, MailConfig, McpConfig, ObjectStorageConfig,
    ObservabilityConfig, PasetoConfig, PasetoDownloadConfig, PasetoDownloadKey,
    PasetoMcpOAuthConfig, PasetoMcpOAuthKey, PasetoUploadGrantConfig, PasetoUploadGrantKey,
    PubSubConfig, PubSubSubscriptionsConfig, ScannerConfig, UploadsConfig, WorkerConfig,
    WorkspaceInvitationPasetoKey, WorkspaceInvitationsConfig,
};

#[derive(Debug, Deserialize)]
pub(super) struct RawAppConfig {
    pub(super) server: RawServerConfig,
    pub(super) database: RawDatabaseConfig,
    pub(super) pubsub: RawPubSubConfig,
    pub(super) auth0: RawAuth0Config,
    pub(super) paseto: RawPasetoConfig,
    pub(super) workspace_invitations: RawWorkspaceInvitationsConfig,
    pub(super) mail: RawMailConfig,
    pub(super) object_storage: RawObjectStorageConfig,
    pub(super) scanner: RawScannerConfig,
    pub(super) uploads: RawUploadsConfig,
    pub(super) observability: RawObservabilityConfig,
    pub(super) worker: RawWorkerConfig,
    pub(super) mcp: RawMcpConfig,
    pub(super) health: RawHealthConfig,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "adapter", rename_all = "snake_case")]
pub(super) enum RawMailConfig {
    LocalStdout,
    Resend { api_key: SecretString, from: String },
}

impl RawMailConfig {
    pub(super) fn validate(self) -> Validation<MailConfig, ConfigFieldError> {
        match self {
            Self::LocalStdout => Validation::valid(MailConfig {
                adapter: MailAdapterConfig::LocalStdout,
            }),
            Self::Resend { api_key, from } => {
                let raw_api_key = api_key;
                let raw_from = from;
                validate! {
                api_key <- secret_value(raw_api_key).at("mail.api_key"),
                from <- string_value(raw_from).at("mail.from"),
                => MailConfig {
                    adapter: MailAdapterConfig::Resend { api_key, from },
                },
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawWorkspaceInvitationsConfig {
    landing_portal_base_url: String,
    active_key_id: String,
    keys: Vec<RawWorkspaceInvitationPasetoKey>,
}

#[derive(Debug, Deserialize)]
struct RawWorkspaceInvitationPasetoKey {
    id: String,
    secret: SecretString,
}

impl RawWorkspaceInvitationsConfig {
    pub(super) fn validate(self) -> Validation<WorkspaceInvitationsConfig, ConfigFieldError> {
        let mut errors = Vec::new();
        let landing_portal_base_url =
            match validate_public_api_base_url(self.landing_portal_base_url)
                .at("workspace_invitations.landing_portal_base_url")
            {
                Validation::Valid(value) => Some(value),
                Validation::Invalid(mut value) => {
                    errors.append(&mut value);
                    None
                }
            };
        let active_key_id = self.active_key_id.trim().to_owned();
        if active_key_id.is_empty() {
            errors.push(ConfigFieldError::new(
                "workspace_invitations.active_key_id",
                "must not be blank",
            ));
        }
        let mut keys = Vec::new();
        for (index, key) in self.keys.into_iter().enumerate() {
            let id = key.id.trim().to_owned();
            if id.is_empty() {
                errors.push(ConfigFieldError::new(
                    format!("workspace_invitations.keys[{index}].id"),
                    "must not be blank",
                ));
            }
            match paseto_download_key(key.secret) {
                Ok(secret) => keys.push(WorkspaceInvitationPasetoKey { id, secret }),
                Err(message) => errors.push(ConfigFieldError::new(
                    format!("workspace_invitations.keys[{index}].secret"),
                    message,
                )),
            }
        }
        if keys.is_empty() {
            errors.push(ConfigFieldError::new(
                "workspace_invitations.keys",
                "must contain at least one key",
            ));
        }
        add_duplicate_id_errors(
            "workspace_invitations.keys",
            keys.iter().map(|key| key.id.as_str()),
            &mut errors,
        );
        if !keys.iter().any(|key| key.id == active_key_id) {
            errors.push(ConfigFieldError::new(
                "workspace_invitations.active_key_id",
                "must exist in workspace_invitations.keys",
            ));
        }
        match (landing_portal_base_url, errors.is_empty()) {
            (Some(landing_portal_base_url), true) => {
                Validation::valid(WorkspaceInvitationsConfig {
                    landing_portal_base_url,
                    active_key_id,
                    keys,
                })
            }
            _ => Validation::invalid_many(errors),
        }
    }
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

#[derive(Debug, Deserialize)]
pub(super) struct RawDatabaseConfig {
    url: SecretString,
    tls: String,
    /// Absent for a plaintext connection, and for a server whose certificate
    /// already chains to a root in the system store.
    #[serde(default)]
    tls_root_certificate: Option<String>,
    pool: RawDatabasePoolConfig,
}

impl RawDatabaseConfig {
    pub(super) fn validate(self) -> Validation<DatabaseConfig, ConfigFieldError> {
        // Named apart from the binding below: `validate!` declares the binding
        // before it evaluates the expression, so a shared name resolves to the
        // empty binding rather than to this.
        let validated_certificate = match self.tls_root_certificate {
            Some(value) => database_root_certificate(value)
                .at("database.tls_root_certificate")
                .map(Some),
            None => Validation::valid(None),
        };

        validate! {
            url <- postgres_connection_string(self.url).at("database.url"),
            mode <- database_tls(self.tls).at("database.tls"),
            root_certificate <- validated_certificate,
            pool <- self.pool.validate(),
            => DatabaseConfig {
                url,
                tls: DatabaseTlsConfig { mode, root_certificate },
                pool,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawDatabasePoolConfig {
    api: usize,
    mcp: usize,
    worker: usize,
    dequeuer: usize,
    acquire_timeout_ms: u64,
    idle_timeout_ms: u64,
}

impl RawDatabasePoolConfig {
    pub(super) fn validate(self) -> Validation<DatabasePoolConfig, ConfigFieldError> {
        validate! {
            api <- nonzero_usize(self.api).at("database.pool.api"),
            mcp <- nonzero_usize(self.mcp).at("database.pool.mcp"),
            worker <- nonzero_usize(self.worker).at("database.pool.worker"),
            dequeuer <- nonzero_usize(self.dequeuer).at("database.pool.dequeuer"),
            acquire_timeout_ms <- nonzero_u64(self.acquire_timeout_ms)
                .at("database.pool.acquire_timeout_ms"),
            idle_timeout_ms <- nonzero_u64(self.idle_timeout_ms)
                .at("database.pool.idle_timeout_ms"),
            => DatabasePoolConfig {
                api,
                mcp,
                worker,
                dequeuer,
                acquire_timeout_ms,
                idle_timeout_ms,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawAuth0Config {
    issuer: String,
    audience: String,
    jwks_url: String,
    upstream_oauth: RawAuth0UpstreamOAuthConfig,
    #[serde(default)]
    auditor_portal: RawAuth0AuditorPortalConfig,
}

#[derive(Debug, Deserialize)]
pub(super) struct RawAuth0UpstreamOAuthConfig {
    client_id: String,
    client_secret: SecretString,
    callback_path: String,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct RawAuth0AuditorPortalConfig {
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    client_secret: Option<SecretString>,
    #[serde(default)]
    callback_path: String,
    #[serde(default)]
    connection: String,
}

pub(super) struct UnresolvedAuth0Config {
    issuer: url::Url,
    audience: String,
    jwks_url: url::Url,
    upstream_oauth: Auth0UpstreamOAuthConfig,
    auditor_portal: UnresolvedAuth0AuditorPortalConfig,
}

struct UnresolvedAuth0AuditorPortalConfig {
    client_id: String,
    client_secret: SecretString,
    callback_path: String,
    connection: String,
}

impl UnresolvedAuth0Config {
    pub(super) fn resolve(self, public_api_base_url: &url::Url) -> Auth0Config {
        let mut callback_url = public_api_base_url.clone();
        callback_url.set_path(&self.auditor_portal.callback_path);

        let mut authorization_endpoint = self.issuer.clone();
        authorization_endpoint.set_path("/authorize");

        let mut token_endpoint = self.issuer.clone();
        token_endpoint.set_path("/oauth/token");

        Auth0Config {
            issuer: self.issuer,
            audience: self.audience,
            jwks_url: self.jwks_url,
            upstream_oauth: self.upstream_oauth,
            auditor_portal: Auth0AuditorPortalConfig {
                client_id: self.auditor_portal.client_id,
                client_secret: self.auditor_portal.client_secret,
                callback_path: self.auditor_portal.callback_path,
                callback_url,
                connection: self.auditor_portal.connection,
                authorization_endpoint,
                token_endpoint,
            },
        }
    }
}

impl RawAuth0Config {
    pub(super) fn validate(self) -> Validation<UnresolvedAuth0Config, ConfigFieldError> {
        validate! {
            issuer <- string_url(self.issuer).at("auth0.issuer"),
            audience <- string_value(self.audience).at("auth0.audience"),
            jwks_url <- string_url(self.jwks_url).at("auth0.jwks_url"),
            upstream_oauth <- self.upstream_oauth.validate(),
            auditor_portal <- self.auditor_portal.validate(),
            => UnresolvedAuth0Config {
                issuer,
                audience,
                jwks_url,
                upstream_oauth,
                auditor_portal,
            },
        }
    }
}

impl RawAuth0UpstreamOAuthConfig {
    pub(super) fn validate(self) -> Validation<Auth0UpstreamOAuthConfig, ConfigFieldError> {
        validate! {
            client_id <- string_value(self.client_id).at("auth0.upstream_oauth.client_id"),
            client_secret <- secret_value(self.client_secret)
                .at("auth0.upstream_oauth.client_secret"),
            callback_path <- path_string(self.callback_path)
                .at("auth0.upstream_oauth.callback_path"),
            => Auth0UpstreamOAuthConfig {
                client_id,
                client_secret,
                callback_path,
            },
        }
    }
}

impl RawAuth0AuditorPortalConfig {
    fn validate(self) -> Validation<UnresolvedAuth0AuditorPortalConfig, ConfigFieldError> {
        validate! {
            client_id <- string_value(self.client_id).at("auth0.auditor_portal.client_id"),
            client_secret <- secret_value(
                self.client_secret.unwrap_or_else(|| SecretString::from(""))
            )
                .at("auth0.auditor_portal.client_secret"),
            callback_path <- auditor_callback_path(self.callback_path)
                .at("auth0.auditor_portal.callback_path"),
            connection <- auditor_connection(self.connection)
                .at("auth0.auditor_portal.connection"),
            => UnresolvedAuth0AuditorPortalConfig {
                client_id,
                client_secret,
                callback_path,
                connection,
            },
        }
    }
}

fn canonical_url(value: String) -> Result<url::Url, String> {
    let url = url::Url::parse(&value).map_err(|_| "must be a valid absolute URL".to_owned())?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err("must not contain a query or fragment".to_owned());
    }
    Ok(url)
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPasetoConfig {
    download: RawPasetoDownloadConfig,
    upload_grant: RawPasetoUploadGrantConfig,
    mcp_oauth: RawPasetoMcpOAuthConfig,
}

impl RawPasetoConfig {
    pub(super) fn validate(self) -> Validation<PasetoConfig, ConfigFieldError> {
        validate! {
            download <- self.download.validate(),
            upload_grant <- self.upload_grant.validate(),
            mcp_oauth <- self.mcp_oauth.validate(),
            => PasetoConfig {
                download,
                upload_grant,
                mcp_oauth,
            },
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

#[derive(Debug, Deserialize)]
pub(super) struct RawPasetoUploadGrantConfig {
    active_key_id: String,
    keys: Vec<RawPasetoUploadGrantKey>,
}

impl RawPasetoUploadGrantConfig {
    pub(super) fn validate(self) -> Validation<PasetoUploadGrantConfig, ConfigFieldError> {
        validate! {
            active_key_id <- string_value(self.active_key_id)
                .at("paseto.upload_grant.active_key_id"),
            keys <- validate_upload_grant_keys(self.keys),
            => PasetoUploadGrantConfig {
                active_key_id,
                keys,
            },
        }
        .and_then(validate_upload_grant_keyring)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPasetoUploadGrantKey {
    id: String,
    secret: SecretString,
}

impl RawPasetoUploadGrantKey {
    fn validate(self, index: usize) -> Validation<PasetoUploadGrantKey, ConfigFieldError> {
        let id_path = format!("paseto.upload_grant.keys[{index}].id");
        let secret_path = format!("paseto.upload_grant.keys[{index}].secret");

        validate! {
            id <- string_value(self.id).at(id_path),
            secret <- paseto_download_key(self.secret).at(secret_path),
            => PasetoUploadGrantKey { id, secret },
        }
    }
}

fn validate_upload_grant_keys(
    keys: Vec<RawPasetoUploadGrantKey>,
) -> Validation<Vec<PasetoUploadGrantKey>, ConfigFieldError> {
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
            "paseto.upload_grant.keys",
            "must contain at least one key",
        ));
    }

    add_duplicate_id_errors(
        "paseto.upload_grant.keys",
        validated.iter().map(|key| key.id.as_str()),
        &mut errors,
    );

    if errors.is_empty() {
        return Validation::valid(validated);
    }

    Validation::invalid_many(errors)
}

fn validate_upload_grant_keyring(
    config: PasetoUploadGrantConfig,
) -> Validation<PasetoUploadGrantConfig, ConfigFieldError> {
    if config.keys.iter().any(|key| key.id == config.active_key_id) {
        return Validation::valid(config);
    }

    Validation::invalid(ConfigFieldError::new(
        "paseto.upload_grant.active_key_id",
        "must exist in paseto.upload_grant.keys",
    ))
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPasetoMcpOAuthConfig {
    active_key_id: String,
    keys: Vec<RawPasetoMcpOAuthKey>,
}

impl RawPasetoMcpOAuthConfig {
    pub(super) fn validate(self) -> Validation<PasetoMcpOAuthConfig, ConfigFieldError> {
        validate! {
            active_key_id <- string_value(self.active_key_id)
                .at("paseto.mcp_oauth.active_key_id"),
            keys <- validate_mcp_oauth_keys(self.keys),
            => PasetoMcpOAuthConfig {
                active_key_id,
                keys,
            },
        }
        .and_then(validate_mcp_oauth_keyring)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct RawPasetoMcpOAuthKey {
    id: String,
    secret: SecretString,
}

impl RawPasetoMcpOAuthKey {
    fn validate(self, index: usize) -> Validation<PasetoMcpOAuthKey, ConfigFieldError> {
        let id_path = format!("paseto.mcp_oauth.keys[{index}].id");
        let secret_path = format!("paseto.mcp_oauth.keys[{index}].secret");

        validate! {
            id <- string_value(self.id).at(id_path),
            secret <- paseto_download_key(self.secret).at(secret_path),
            => PasetoMcpOAuthKey { id, secret },
        }
    }
}

fn validate_mcp_oauth_keys(
    keys: Vec<RawPasetoMcpOAuthKey>,
) -> Validation<Vec<PasetoMcpOAuthKey>, ConfigFieldError> {
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
            "paseto.mcp_oauth.keys",
            "must contain at least one key",
        ));
    }

    add_duplicate_id_errors(
        "paseto.mcp_oauth.keys",
        validated.iter().map(|key| key.id.as_str()),
        &mut errors,
    );

    if errors.is_empty() {
        return Validation::valid(validated);
    }

    Validation::invalid_many(errors)
}

fn validate_mcp_oauth_keyring(
    config: PasetoMcpOAuthConfig,
) -> Validation<PasetoMcpOAuthConfig, ConfigFieldError> {
    if config.keys.iter().any(|key| key.id == config.active_key_id) {
        return Validation::valid(config);
    }

    Validation::invalid(ConfigFieldError::new(
        "paseto.mcp_oauth.active_key_id",
        "must exist in paseto.mcp_oauth.keys",
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
        quarantine_root: String,
        evidence_root: String,
    },
    Gcs {
        quarantine_bucket: String,
        evidence_bucket: String,
        object_key_prefix: String,
    },
}

impl RawObjectStorageConfig {
    pub(super) fn validate(self) -> Validation<ObjectStorageConfig, ConfigFieldError> {
        match self {
            Self::Filesystem {
                quarantine_root: raw_quarantine_root,
                evidence_root: raw_evidence_root,
            } => validate! {
                quarantine_root <- string_value(raw_quarantine_root)
                    .at("object_storage.quarantine_root"),
                evidence_root <- string_value(raw_evidence_root)
                    .at("object_storage.evidence_root"),
                => FilesystemObjectStorageConfig {
                    quarantine_root: PathBuf::from(quarantine_root),
                    evidence_root: PathBuf::from(evidence_root),
                },
            }
            .and_then(|config| {
                if config.quarantine_root == config.evidence_root {
                    return separate_targets_error(
                        "object_storage.evidence_root",
                        "object_storage.quarantine_root",
                    );
                }
                Validation::valid(ObjectStorageConfig::Filesystem(config))
            }),
            Self::Gcs {
                quarantine_bucket: raw_quarantine_bucket,
                evidence_bucket: raw_evidence_bucket,
                object_key_prefix: raw_object_key_prefix,
            } => validate! {
                quarantine_bucket <- string_value(raw_quarantine_bucket)
                    .at("object_storage.quarantine_bucket"),
                evidence_bucket <- string_value(raw_evidence_bucket)
                    .at("object_storage.evidence_bucket"),
                object_key_prefix <- validate_object_key_prefix(raw_object_key_prefix)
                    .at("object_storage.object_key_prefix"),
                => GcsObjectStorageConfig {
                    quarantine_bucket,
                    evidence_bucket,
                    object_key_prefix,
                },
            }
            .and_then(|config| {
                if config.quarantine_bucket == config.evidence_bucket {
                    return separate_targets_error(
                        "object_storage.evidence_bucket",
                        "object_storage.quarantine_bucket",
                    );
                }
                Validation::valid(ObjectStorageConfig::Gcs(config))
            }),
        }
    }
}

/// One target for both stores puts unscanned bytes where the evidence lives and
/// exposes the evidence to the quarantine expiry rule. Refuse to start.
fn separate_targets_error<T>(path: &str, other_path: &str) -> Validation<T, ConfigFieldError> {
    Validation::invalid(ConfigFieldError::new(
        path,
        format!("must not name the same target as {other_path}"),
    ))
}

#[derive(Debug, Deserialize)]
pub(super) struct RawUploadsConfig {
    max_document_bytes: usize,
}

impl RawUploadsConfig {
    pub(super) fn validate(self) -> Validation<UploadsConfig, ConfigFieldError> {
        nonzero_usize(self.max_document_bytes)
            .at("uploads.max_document_bytes")
            .map(|max_document_bytes| UploadsConfig { max_document_bytes })
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
    resource: String,
    #[serde(default)]
    allowed_hosts: Vec<String>,
}

impl RawMcpConfig {
    pub(super) fn validate(self) -> Validation<McpConfig, ConfigFieldError> {
        validate! {
            shutdown_grace_seconds <- nonzero_u64(self.shutdown_grace_seconds)
                .at("mcp.shutdown_grace_seconds"),
            resource <- canonical_url(self.resource).at("mcp.resource"),
            allowed_hosts <- Validation::valid(self.allowed_hosts),
            => McpConfig {
                shutdown_grace_seconds,
                resource,
                allowed_hosts,
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
