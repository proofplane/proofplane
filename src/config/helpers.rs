// TODO(low priority): rewrite helpers that can return early instead of using `else`.

use std::net::SocketAddr;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use pasetors::{
    keys::{AsymmetricPublicKey, AsymmetricSecretKey, SymmetricKey},
    paserk::FormatAsPaserk,
    version4::V4,
};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::validation::Validation;

use super::{ConfigFieldError, GcsCredentialsMode, LogFormat};

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

pub(super) fn public_api_base_url(value: String) -> Result<Url, String> {
    let url = string_url(value)?;
    let is_loopback = url
        .host_str()
        .and_then(|host| host.parse::<std::net::IpAddr>().ok())
        .is_some_and(|address| address.is_loopback())
        || matches!(url.host_str(), Some("localhost"));

    if url.scheme() != "https" && !(url.scheme() == "http" && is_loopback) {
        return Err("must use HTTPS except for loopback development URLs".into());
    }
    if url.cannot_be_a_base()
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("must be an origin URL without credentials, path, query, or fragment".into());
    }

    Ok(url)
}

pub(super) fn download_signing_secret(value: SecretString) -> Result<SecretString, String> {
    let encoded = secret_value(value)?;
    let decoded = BASE64_STANDARD
        .decode(encoded.expose_secret())
        .map_err(|_| "must be valid base64".to_owned())?;
    if decoded.len() < 32 {
        return Err("must decode to at least 32 bytes".into());
    }

    Ok(encoded)
}

pub(super) fn paseto_api_secret_key(value: SecretString) -> Result<SecretString, String> {
    secret_value(value).and_then(|value| {
        AsymmetricSecretKey::<V4>::try_from(value.expose_secret())
            .map(|_| value)
            .map_err(|_| "must be a valid k4.secret PASERK".into())
    })
}

pub(super) fn paseto_api_public_key(value: String) -> Result<String, String> {
    let value = trim_required(value)?;
    AsymmetricPublicKey::<V4>::try_from(value.as_str())
        .map(|_| value)
        .map_err(|_| "must be a valid k4.public PASERK".into())
}

pub(super) fn paseto_download_key(value: SecretString) -> Result<SecretString, String> {
    secret_value(value).and_then(|value| {
        SymmetricKey::<V4>::try_from(value.expose_secret())
            .map(|_| value)
            .map_err(|_| "must be a valid k4.local PASERK".into())
    })
}

pub(super) fn paseto_public_from_secret(value: &SecretString) -> Result<String, String> {
    let secret = AsymmetricSecretKey::<V4>::try_from(value.expose_secret())
        .map_err(|_| "must be a valid k4.secret PASERK".to_owned())?;
    let public = AsymmetricPublicKey::<V4>::try_from(&secret)
        .map_err(|_| "must derive a k4.public key".to_owned())?;
    let mut formatted = String::new();
    public
        .fmt(&mut formatted)
        .map_err(|_| "must format derived public key".to_owned())?;

    Ok(formatted)
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
    fn at(self, path: impl Into<String>) -> Validation<T, ConfigFieldError>;
}

impl<T> ConfigValidationExt<T> for Result<T, String> {
    fn at(self, path: impl Into<String>) -> Validation<T, ConfigFieldError> {
        match self {
            Ok(value) => Validation::valid(value),
            Err(message) => Validation::invalid(ConfigFieldError::new(path, message)),
        }
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::{download_signing_secret, public_api_base_url};

    #[test]
    fn public_api_base_url_requires_https_except_loopback_origins() {
        assert!(public_api_base_url("https://api.proofplane.com".to_owned()).is_ok());
        assert!(public_api_base_url("http://127.0.0.1:3000".to_owned()).is_ok());
        assert!(public_api_base_url("http://localhost:3000".to_owned()).is_ok());
        assert!(public_api_base_url("http://api.proofplane.com".to_owned()).is_err());
        assert!(public_api_base_url("https://api.proofplane.com/v1".to_owned()).is_err());
        assert!(public_api_base_url("https://user@example.com".to_owned()).is_err());
    }

    #[test]
    fn download_signing_secret_requires_valid_base64_with_32_decoded_bytes() {
        assert!(download_signing_secret("not-base64".into()).is_err());
        assert!(download_signing_secret("c2hvcnQ=".into()).is_err());

        let secret = download_signing_secret("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=".into())
            .expect("32-byte secret validates");
        let debug = format!("{secret:?}");
        assert!(!debug.contains(secret.expose_secret()));
        assert!(debug.contains("Secret"));
    }
}
