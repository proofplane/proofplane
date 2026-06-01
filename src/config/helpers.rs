// TODO(low priority): rewrite helpers that can return early instead of using `else`.

use std::net::SocketAddr;

use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::validation::Validation;

use super::{ConfigFieldError, GcsCredentialsMode, HostPort, LogFormat};

pub(super) fn string_value(value: String) -> Result<String, String> {
    trim_required(value)
}

pub(super) fn secret_value(value: SecretString) -> Result<SecretString, String> {
    if value.expose_secret().trim().is_empty() {
        Err("must not be empty".into())
    } else {
        Ok(value)
    }
}

pub(super) fn postgres_connection_string(value: SecretString) -> Result<SecretString, String> {
    let value = secret_value(value)?;

    value
        .expose_secret()
        .parse::<tokio_postgres::Config>()
        .map(|_| value)
        .map_err(|_| "must be a valid Postgres connection string".into())
}

pub(super) fn nonzero_u16(value: u16) -> Result<u16, String> {
    if value == 0 {
        Err("must be greater than zero".into())
    } else {
        Ok(value)
    }
}

pub(super) fn nonzero_u64(value: u64) -> Result<u64, String> {
    if value == 0 {
        Err("must be greater than zero".into())
    } else {
        Ok(value)
    }
}

pub(super) fn nonzero_usize(value: usize) -> Result<usize, String> {
    if value == 0 {
        Err("must be greater than zero".into())
    } else {
        Ok(value)
    }
}

pub(super) fn socket_addr(value: String) -> Result<SocketAddr, String> {
    if value.trim().is_empty() {
        return Err("must not be empty".into());
    }

    value
        .trim()
        .to_owned()
        .parse::<SocketAddr>()
        .map_err(|_| "must be a socket address".into())
}

pub(super) fn host_port(value: String) -> Result<HostPort, String> {
    let value = trim_required(value)?;
    let (host, port) = value
        .rsplit_once(':')
        .ok_or_else(|| "must be a host and port like 127.0.0.1:8085".to_owned())?;

    validate_hostname(host)?;

    let port = port
        .parse::<u16>()
        .map_err(|_| "port must be a number between 1 and 65535".to_owned())?;

    if port == 0 {
        return Err("port must be greater than zero".into());
    }

    Ok(HostPort {
        host: host.to_owned(),
        port,
    })
}

pub(super) fn validate_hostname(host: &str) -> Result<(), String> {
    if host.is_empty() {
        return Err("host must not be empty".into());
    }

    for label in host.split('.') {
        if label.is_empty() {
            return Err("host labels must not be empty".into());
        }

        if label.starts_with('-') || label.ends_with('-') {
            return Err("host labels must not start or end with `-`".into());
        }

        if !label
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return Err("host must contain only ASCII letters, digits, hyphens, or dots".into());
        }
    }

    Ok(())
}

pub(super) fn optional_url(value: Option<String>) -> Result<Option<Url>, String> {
    match value {
        Some(value) if value.trim().is_empty() => Err("must not be empty when set".into()),
        Some(value) => Url::parse(value.trim())
            .map(Some)
            .map_err(|_| "must be a valid URL".into()),
        None => Ok(None),
    }
}

pub(super) fn string_url(value: String) -> Result<Url, String> {
    Url::parse(trim_required(value)?.as_str()).map_err(|_| "must be a valid URL".into())
}

pub(super) fn parse_log_format(value: String) -> Result<LogFormat, String> {
    match trim_required(value)?.as_str() {
        "json" => Ok(LogFormat::Json),
        "pretty" => Ok(LogFormat::Pretty),
        _ => Err("must be `json` or `pretty`".into()),
    }
}

pub(super) fn gcs_credentials_mode(value: String) -> Result<GcsCredentialsMode, String> {
    match trim_required(value)?.as_str() {
        "application_default" => Ok(GcsCredentialsMode::ApplicationDefault),
        "anonymous" => Ok(GcsCredentialsMode::Anonymous),
        _ => Err("must be `application_default` or `anonymous`".into()),
    }
}

pub(super) fn path_string(value: String) -> Result<String, String> {
    let value = trim_required(value)?;

    if value.starts_with('/') {
        Ok(value)
    } else {
        Err("must start with `/`".into())
    }
}

pub(super) fn trim_required(value: String) -> Result<String, String> {
    if value.trim().is_empty() {
        Err("must not be empty".into())
    } else {
        Ok(value.trim().to_owned())
    }
}

pub(super) trait ConfigValidationExt<T> {
    fn at(self, path: &'static str) -> Validation<T, ConfigFieldError>;
}

impl<T> ConfigValidationExt<T> for Result<T, String> {
    fn at(self, path: &'static str) -> Validation<T, ConfigFieldError> {
        match self {
            Ok(value) => Validation::valid(value),
            Err(message) => Validation::invalid(ConfigFieldError::new(path, message)),
        }
    }
}
