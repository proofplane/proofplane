use std::collections::HashMap;

use chrono::{DateTime, SecondsFormat, Utc};
use pasetors::{
    keys::SymmetricKey,
    token::{Local, UntrustedToken},
    version4::{LocalToken, V4},
};
use secrecy::ExposeSecret;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{Map, Value};
use url::Url;
use uuid::Uuid;

use crate::config::{PasetoDownloadConfig, PasetoMcpOAuthConfig, PasetoUploadGrantConfig};

// We use assertions as a mechanism for verifying that the API key is being
// used for the right purpose. For example, for API keys, we want to make sure
// that the tokens we're verifying were issued with the intention of them
// being API keys and not something else.
const DOWNLOAD_IMPLICIT_ASSERTION: &[u8] = b"proofplane:submission-download:v1";
const UPLOAD_GRANT_IMPLICIT_ASSERTION: &[u8] = b"proofplane:evidence-upload-grant:v1";
const UPLOAD_SESSION_IMPLICIT_ASSERTION: &[u8] = b"proofplane:evidence-upload-session:v1";
const MCP_OAUTH_IMPLICIT_ASSERTION: &[u8] = b"proofplane:mcp-oauth-access:v1";
const REGISTERED_CLAIMS: [&str; 7] = ["iss", "aud", "sub", "jti", "iat", "nbf", "exp"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredClaims {
    pub subject: Uuid,
    pub token_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedPasetoToken {
    pub token: String,
    pub token_id: Uuid,
    pub key_id: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPasetoToken<T> {
    pub subject: Uuid,
    pub token_id: Uuid,
    pub key_id: String,
    pub expires_at: DateTime<Utc>,
    pub claims: T,
}

#[derive(Clone)]
pub struct DownloadGrantEncryptor {
    issuer: Url,
    audience: String,
    key_id: String,
    secret_key: SymmetricKey<V4>,
}

impl DownloadGrantEncryptor {
    pub fn from_config(
        issuer: Url,
        audience: impl Into<String>,
        config: &PasetoDownloadConfig,
    ) -> Result<Self, Error> {
        let active = config
            .keys
            .iter()
            .find(|key| key.id == config.active_key_id)
            .ok_or(Error::Keyring)?;

        Ok(Self {
            issuer,
            audience: audience.into(),
            key_id: active.id.clone(),
            secret_key: symmetric_key(active.secret.expose_secret())?,
        })
    }

    pub fn encrypt<T: Serialize>(
        &self,
        registered: RegisteredClaims,
        custom_claims: &T,
    ) -> Result<IssuedPasetoToken, Error> {
        let payload = payload(
            self.issuer.as_str(),
            &self.audience,
            &registered,
            custom_claims,
        )?;
        let footer = footer(&self.key_id)?;
        let token = encrypt_local_token(
            &self.secret_key,
            payload.as_bytes(),
            footer.as_bytes(),
            DOWNLOAD_IMPLICIT_ASSERTION,
        )?;

        Ok(IssuedPasetoToken {
            token,
            token_id: registered.token_id,
            key_id: self.key_id.clone(),
            expires_at: normalize_datetime(registered.expires_at)?,
        })
    }
}

#[derive(Clone)]
pub struct DownloadGrantDecryptor {
    issuer: Url,
    audience: String,
    keys: HashMap<String, SymmetricKey<V4>>,
}

impl DownloadGrantDecryptor {
    pub fn from_config(
        issuer: Url,
        audience: impl Into<String>,
        config: &PasetoDownloadConfig,
    ) -> Result<Self, Error> {
        let mut keys = HashMap::new();
        for key in &config.keys {
            keys.insert(key.id.clone(), symmetric_key(key.secret.expose_secret())?);
        }

        Ok(Self {
            issuer,
            audience: audience.into(),
            keys,
        })
    }

    pub fn decrypt<T: DeserializeOwned>(
        &self,
        token: &str,
    ) -> Result<VerifiedPasetoToken<T>, Error> {
        let untrusted = UntrustedToken::<Local, V4>::try_from(token).map_err(|_| Error::Verify)?;
        let footer = parse_footer(untrusted.untrusted_footer())?;
        let key = self.keys.get(&footer.kid).ok_or(Error::Verify)?;
        let trusted = decrypt_local_token(key, &untrusted, DOWNLOAD_IMPLICIT_ASSERTION)?;

        verified_payload(
            trusted.payload(),
            &footer.kid,
            self.issuer.as_str(),
            &self.audience,
        )
    }
}

#[derive(Clone)]
pub struct UploadGrantEncryptor {
    issuer: Url,
    audience: String,
    key_id: String,
    secret_key: SymmetricKey<V4>,
}

impl UploadGrantEncryptor {
    pub fn from_config(
        issuer: Url,
        audience: impl Into<String>,
        config: &PasetoUploadGrantConfig,
    ) -> Result<Self, Error> {
        let active = config
            .keys
            .iter()
            .find(|key| key.id == config.active_key_id)
            .ok_or(Error::Keyring)?;

        Ok(Self {
            issuer,
            audience: audience.into(),
            key_id: active.id.clone(),
            secret_key: symmetric_key(active.secret.expose_secret())?,
        })
    }

    pub fn encrypt<T: Serialize>(
        &self,
        registered: RegisteredClaims,
        custom_claims: &T,
    ) -> Result<IssuedPasetoToken, Error> {
        let payload = payload(
            self.issuer.as_str(),
            &self.audience,
            &registered,
            custom_claims,
        )?;
        let footer = footer(&self.key_id)?;
        let token = encrypt_local_token(
            &self.secret_key,
            payload.as_bytes(),
            footer.as_bytes(),
            UPLOAD_GRANT_IMPLICIT_ASSERTION,
        )?;

        Ok(IssuedPasetoToken {
            token,
            token_id: registered.token_id,
            key_id: self.key_id.clone(),
            expires_at: normalize_datetime(registered.expires_at)?,
        })
    }
}

#[derive(Clone)]
pub struct UploadGrantDecryptor {
    issuer: Url,
    audience: String,
    keys: HashMap<String, SymmetricKey<V4>>,
}

impl UploadGrantDecryptor {
    pub fn from_config(
        issuer: Url,
        audience: impl Into<String>,
        config: &PasetoUploadGrantConfig,
    ) -> Result<Self, Error> {
        let mut keys = HashMap::new();
        for key in &config.keys {
            keys.insert(key.id.clone(), symmetric_key(key.secret.expose_secret())?);
        }

        Ok(Self {
            issuer,
            audience: audience.into(),
            keys,
        })
    }

    pub fn decrypt<T: DeserializeOwned>(
        &self,
        token: &str,
    ) -> Result<VerifiedPasetoToken<T>, Error> {
        let untrusted = UntrustedToken::<Local, V4>::try_from(token).map_err(|_| Error::Verify)?;
        let footer = parse_footer(untrusted.untrusted_footer())?;
        let key = self.keys.get(&footer.kid).ok_or(Error::Verify)?;
        let trusted = decrypt_local_token(key, &untrusted, UPLOAD_GRANT_IMPLICIT_ASSERTION)?;

        verified_payload(
            trusted.payload(),
            &footer.kid,
            self.issuer.as_str(),
            &self.audience,
        )
    }
}

#[derive(Clone)]
pub struct UploadSessionEncryptor {
    issuer: Url,
    audience: String,
    key_id: String,
    secret_key: SymmetricKey<V4>,
}

impl UploadSessionEncryptor {
    pub fn from_config(
        issuer: Url,
        audience: impl Into<String>,
        config: &PasetoUploadGrantConfig,
    ) -> Result<Self, Error> {
        let active = config
            .keys
            .iter()
            .find(|key| key.id == config.active_key_id)
            .ok_or(Error::Keyring)?;

        Ok(Self {
            issuer,
            audience: audience.into(),
            key_id: active.id.clone(),
            secret_key: symmetric_key(active.secret.expose_secret())?,
        })
    }

    pub fn encrypt<T: Serialize>(
        &self,
        registered: RegisteredClaims,
        custom_claims: &T,
    ) -> Result<IssuedPasetoToken, Error> {
        let payload = payload(
            self.issuer.as_str(),
            &self.audience,
            &registered,
            custom_claims,
        )?;
        let footer = footer(&self.key_id)?;
        let token = encrypt_local_token(
            &self.secret_key,
            payload.as_bytes(),
            footer.as_bytes(),
            UPLOAD_SESSION_IMPLICIT_ASSERTION,
        )?;

        Ok(IssuedPasetoToken {
            token,
            token_id: registered.token_id,
            key_id: self.key_id.clone(),
            expires_at: normalize_datetime(registered.expires_at)?,
        })
    }
}

#[derive(Clone)]
pub struct UploadSessionDecryptor {
    issuer: Url,
    audience: String,
    keys: HashMap<String, SymmetricKey<V4>>,
}

#[derive(Clone)]
pub struct McpOAuthEncryptor {
    issuer: Url,
    audience: String,
    key_id: String,
    secret_key: SymmetricKey<V4>,
}

impl McpOAuthEncryptor {
    pub fn from_config(
        issuer: Url,
        audience: impl Into<String>,
        config: &PasetoMcpOAuthConfig,
    ) -> Result<Self, Error> {
        let active = config
            .keys
            .iter()
            .find(|key| key.id == config.active_key_id)
            .ok_or(Error::Keyring)?;

        Ok(Self {
            issuer,
            audience: audience.into(),
            key_id: active.id.clone(),
            secret_key: symmetric_key(active.secret.expose_secret())?,
        })
    }

    pub fn encrypt<T: Serialize>(
        &self,
        registered: RegisteredClaims,
        custom_claims: &T,
    ) -> Result<IssuedPasetoToken, Error> {
        let payload = payload(
            self.issuer.as_str(),
            &self.audience,
            &registered,
            custom_claims,
        )?;
        let footer = footer(&self.key_id)?;
        let token = encrypt_local_token(
            &self.secret_key,
            payload.as_bytes(),
            footer.as_bytes(),
            MCP_OAUTH_IMPLICIT_ASSERTION,
        )?;

        Ok(IssuedPasetoToken {
            token,
            token_id: registered.token_id,
            key_id: self.key_id.clone(),
            expires_at: normalize_datetime(registered.expires_at)?,
        })
    }
}

#[derive(Clone)]
pub struct McpOAuthDecryptor {
    issuer: Url,
    audience: String,
    keys: HashMap<String, SymmetricKey<V4>>,
}

impl McpOAuthDecryptor {
    pub fn from_config(
        issuer: Url,
        audience: impl Into<String>,
        config: &PasetoMcpOAuthConfig,
    ) -> Result<Self, Error> {
        let mut keys = HashMap::new();
        for key in &config.keys {
            keys.insert(key.id.clone(), symmetric_key(key.secret.expose_secret())?);
        }

        Ok(Self {
            issuer,
            audience: audience.into(),
            keys,
        })
    }

    pub fn decrypt<T: DeserializeOwned>(
        &self,
        token: &str,
    ) -> Result<VerifiedPasetoToken<T>, Error> {
        let untrusted = UntrustedToken::<Local, V4>::try_from(token).map_err(|_| Error::Verify)?;
        let footer = parse_footer(untrusted.untrusted_footer())?;
        let key = self.keys.get(&footer.kid).ok_or(Error::Verify)?;
        let trusted = decrypt_local_token(key, &untrusted, MCP_OAUTH_IMPLICIT_ASSERTION)?;

        verified_payload(
            trusted.payload(),
            &footer.kid,
            self.issuer.as_str(),
            &self.audience,
        )
    }
}

impl UploadSessionDecryptor {
    pub fn from_config(
        issuer: Url,
        audience: impl Into<String>,
        config: &PasetoUploadGrantConfig,
    ) -> Result<Self, Error> {
        let mut keys = HashMap::new();
        for key in &config.keys {
            keys.insert(key.id.clone(), symmetric_key(key.secret.expose_secret())?);
        }

        Ok(Self {
            issuer,
            audience: audience.into(),
            keys,
        })
    }

    pub fn decrypt<T: DeserializeOwned>(
        &self,
        token: &str,
    ) -> Result<VerifiedPasetoToken<T>, Error> {
        let untrusted = UntrustedToken::<Local, V4>::try_from(token).map_err(|_| Error::Verify)?;
        let footer = parse_footer(untrusted.untrusted_footer())?;
        let key = self.keys.get(&footer.kid).ok_or(Error::Verify)?;
        let trusted = decrypt_local_token(key, &untrusted, UPLOAD_SESSION_IMPLICIT_ASSERTION)?;

        verified_payload(
            trusted.payload(),
            &footer.kid,
            self.issuer.as_str(),
            &self.audience,
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid PASETO keyring")]
    Keyring,
    #[error("PASETO token issue failed")]
    Issue,
    #[error("PASETO token verification failed")]
    Verify,
}

#[derive(Debug, Deserialize, Serialize)]
struct TokenFooter {
    kid: String,
}

fn payload<T: Serialize>(
    issuer: &str,
    audience: &str,
    registered: &RegisteredClaims,
    custom_claims: &T,
) -> Result<String, Error> {
    let issued_at = Utc::now();
    if registered.expires_at <= issued_at {
        return Err(Error::Issue);
    }

    let mut claims = match serde_json::to_value(custom_claims).map_err(|_| Error::Issue)? {
        Value::Object(claims) => Ok(claims),
        _ => Err(Error::Issue),
    }?;

    for claim in REGISTERED_CLAIMS {
        if claims.contains_key(claim) {
            return Err(Error::Issue);
        }
    }

    claims.insert("iss".to_owned(), Value::String(issuer.to_owned()));
    claims.insert("aud".to_owned(), Value::String(audience.to_owned()));
    claims.insert(
        "sub".to_owned(),
        Value::String(registered.subject.to_string()),
    );
    claims.insert(
        "jti".to_owned(),
        Value::String(registered.token_id.to_string()),
    );
    let issued_at = format_datetime(issued_at);
    claims.insert("iat".to_owned(), Value::String(issued_at.clone()));
    claims.insert("nbf".to_owned(), Value::String(issued_at));
    claims.insert(
        "exp".to_owned(),
        Value::String(format_datetime(registered.expires_at)),
    );

    serde_json::to_string(&claims).map_err(|_| Error::Issue)
}

fn verified_payload<T: DeserializeOwned>(
    payload: &str,
    key_id: &str,
    issuer: &str,
    audience: &str,
) -> Result<VerifiedPasetoToken<T>, Error> {
    let mut claims: Map<String, Value> =
        serde_json::from_str(payload).map_err(|_| Error::Verify)?;

    if string_claim(&claims, "iss")? != issuer || string_claim(&claims, "aud")? != audience {
        return Err(Error::Verify);
    }

    let subject = Uuid::parse_str(string_claim(&claims, "sub")?).map_err(|_| Error::Verify)?;
    let token_id = Uuid::parse_str(string_claim(&claims, "jti")?).map_err(|_| Error::Verify)?;
    let issued_at = datetime_claim(&claims, "iat")?;
    let not_before = datetime_claim(&claims, "nbf")?;
    let expires_at = datetime_claim(&claims, "exp")?;
    let now = Utc::now();
    if issued_at > now || not_before > now || expires_at <= now || issued_at >= expires_at {
        return Err(Error::Verify);
    }

    for claim in REGISTERED_CLAIMS {
        claims.remove(claim);
    }

    Ok(VerifiedPasetoToken {
        subject,
        token_id,
        key_id: key_id.to_owned(),
        expires_at,
        claims: serde_json::from_value(Value::Object(claims)).map_err(|_| Error::Verify)?,
    })
}

fn footer(key_id: &str) -> Result<String, Error> {
    serde_json::to_string(&TokenFooter {
        kid: key_id.to_owned(),
    })
    .map_err(|_| Error::Issue)
}

fn symmetric_key(secret: &str) -> Result<SymmetricKey<V4>, Error> {
    SymmetricKey::<V4>::try_from(secret).map_err(|_| Error::Keyring)
}

fn encrypt_local_token(
    key: &SymmetricKey<V4>,
    payload: &[u8],
    footer: &[u8],
    implicit_assertion: &[u8],
) -> Result<String, Error> {
    LocalToken::encrypt(key, payload, Some(footer), Some(implicit_assertion))
        .map_err(|_| Error::Issue)
}

fn decrypt_local_token(
    key: &SymmetricKey<V4>,
    token: &UntrustedToken<Local, V4>,
    implicit_assertion: &[u8],
) -> Result<pasetors::token::TrustedToken, Error> {
    LocalToken::decrypt(
        key,
        token,
        Some(token.untrusted_footer()),
        Some(implicit_assertion),
    )
    .map_err(|_| Error::Verify)
}

fn parse_footer(footer: &[u8]) -> Result<TokenFooter, Error> {
    let footer: TokenFooter = serde_json::from_slice(footer).map_err(|_| Error::Verify)?;
    if footer.kid.trim().is_empty() {
        return Err(Error::Verify);
    }

    Ok(footer)
}

fn string_claim<'a>(claims: &'a Map<String, Value>, claim: &str) -> Result<&'a str, Error> {
    claims
        .get(claim)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(Error::Verify)
}

fn datetime_claim(claims: &Map<String, Value>, claim: &str) -> Result<DateTime<Utc>, Error> {
    DateTime::parse_from_rfc3339(string_claim(claims, claim)?)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| Error::Verify)
}

fn format_datetime(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn normalize_datetime(value: DateTime<Utc>) -> Result<DateTime<Utc>, Error> {
    DateTime::parse_from_rfc3339(&format_datetime(value))
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| Error::Issue)
}

#[cfg(test)]
mod tests {
    use chrono::Duration as ChronoDuration;
    use secrecy::SecretString;

    use super::*;
    use crate::config::{PasetoDownloadKey, PasetoUploadGrantConfig, PasetoUploadGrantKey};

    const DOWNLOAD_AUDIENCE: &str = "proofplane-submission-download";
    const UPLOAD_GRANT_AUDIENCE: &str = "proofplane-evidence-upload-grant";
    const DOWNLOAD_SECRET: &str = "k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs";
    const UPLOAD_GRANT_SECRET: &str = "k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY";
    const OTHER_DOWNLOAD_SECRET: &str = UPLOAD_GRANT_SECRET;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestClaims {
        workspace_id: Uuid,
        permission: String,
    }

    fn issuer() -> Url {
        Url::parse("https://api.proofplane.test/").unwrap()
    }

    fn registered() -> RegisteredClaims {
        RegisteredClaims {
            subject: Uuid::new_v4(),
            token_id: Uuid::new_v4(),
            expires_at: Utc::now() + ChronoDuration::minutes(5),
        }
    }

    fn custom_claims() -> TestClaims {
        TestClaims {
            workspace_id: Uuid::new_v4(),
            permission: "read".to_owned(),
        }
    }

    fn download_config(active_id: &str) -> PasetoDownloadConfig {
        PasetoDownloadConfig {
            active_key_id: active_id.to_owned(),
            keys: vec![
                PasetoDownloadKey {
                    id: "local-download".to_owned(),
                    secret: SecretString::from(DOWNLOAD_SECRET),
                },
                PasetoDownloadKey {
                    id: "old-download".to_owned(),
                    secret: SecretString::from(OTHER_DOWNLOAD_SECRET),
                },
            ],
        }
    }

    fn upload_grant_config(active_id: &str) -> PasetoUploadGrantConfig {
        PasetoUploadGrantConfig {
            active_key_id: active_id.to_owned(),
            keys: vec![PasetoUploadGrantKey {
                id: "local-upload-grant".to_owned(),
                secret: SecretString::from(UPLOAD_GRANT_SECRET),
            }],
        }
    }

    fn download_encryptor(config: &PasetoDownloadConfig) -> DownloadGrantEncryptor {
        DownloadGrantEncryptor::from_config(issuer(), DOWNLOAD_AUDIENCE, config).unwrap()
    }

    fn download_decryptor(config: &PasetoDownloadConfig) -> DownloadGrantDecryptor {
        DownloadGrantDecryptor::from_config(issuer(), DOWNLOAD_AUDIENCE, config).unwrap()
    }

    fn upload_grant_encryptor(config: &PasetoUploadGrantConfig) -> UploadGrantEncryptor {
        UploadGrantEncryptor::from_config(issuer(), UPLOAD_GRANT_AUDIENCE, config).unwrap()
    }

    fn upload_grant_decryptor(config: &PasetoUploadGrantConfig) -> UploadGrantDecryptor {
        UploadGrantDecryptor::from_config(issuer(), UPLOAD_GRANT_AUDIENCE, config).unwrap()
    }

    #[test]
    fn download_local_tokens_round_trip() {
        let config = download_config("local-download");
        let registered = registered();
        let claims = custom_claims();
        let issued = download_encryptor(&config)
            .encrypt(registered.clone(), &claims)
            .unwrap();
        let verified = download_decryptor(&config)
            .decrypt::<TestClaims>(&issued.token)
            .unwrap();

        assert_eq!(verified.token_id, registered.token_id);
        assert_eq!(verified.subject, registered.subject);
        assert_eq!(verified.key_id, "local-download");
        assert_eq!(verified.claims, claims);
        assert_eq!(verified.expires_at, issued.expires_at);
        assert!(issued.token.starts_with("v4.local."));
    }

    #[test]
    fn rejects_malformed_unknown_kid_wrong_key_and_tampering() {
        let download_config = download_config("local-download");
        assert!(download_decryptor(&download_config)
            .decrypt::<TestClaims>("not-a-token")
            .is_err());
        let issued = download_encryptor(&download_config)
            .encrypt(registered(), &custom_claims())
            .unwrap();
        let wrong_key = PasetoDownloadConfig {
            active_key_id: "local-download".to_owned(),
            keys: vec![PasetoDownloadKey {
                id: "local-download".to_owned(),
                secret: SecretString::from(OTHER_DOWNLOAD_SECRET),
            }],
        };
        assert!(download_decryptor(&wrong_key)
            .decrypt::<TestClaims>(&issued.token)
            .is_err());
        assert!(download_decryptor(&download_config)
            .decrypt::<TestClaims>(&tamper(&issued.token))
            .is_err());
        assert!(download_decryptor(&download_config)
            .decrypt::<TestClaims>(&tamper_footer(&issued.token))
            .is_err());
    }

    #[test]
    fn key_rotation_keeps_old_keys_until_removed() {
        let old_download_config = download_config("old-download");
        let download_token = download_encryptor(&old_download_config)
            .encrypt(registered(), &custom_claims())
            .unwrap()
            .token;
        assert!(download_decryptor(&download_config("local-download"))
            .decrypt::<TestClaims>(&download_token)
            .is_ok());
        let removed_old_download = PasetoDownloadConfig {
            active_key_id: "local-download".to_owned(),
            keys: vec![PasetoDownloadKey {
                id: "local-download".to_owned(),
                secret: SecretString::from(DOWNLOAD_SECRET),
            }],
        };
        assert!(download_decryptor(&removed_old_download)
            .decrypt::<TestClaims>(&download_token)
            .is_err());
    }

    #[test]
    fn purpose_and_implicit_assertion_separation_fail_closed() {
        let download_config = download_config("local-download");
        let upload_grant_config = upload_grant_config("local-upload-grant");
        let wrong_audience_decryptor =
            DownloadGrantDecryptor::from_config(issuer(), "wrong-audience", &download_config)
                .unwrap();
        let download_token = download_encryptor(&download_config)
            .encrypt(registered(), &custom_claims())
            .unwrap()
            .token;
        let upload_grant_token = upload_grant_encryptor(&upload_grant_config)
            .encrypt(registered(), &custom_claims())
            .unwrap()
            .token;
        assert!(wrong_audience_decryptor
            .decrypt::<TestClaims>(&download_token)
            .is_err());
        assert!(upload_grant_decryptor(&upload_grant_config)
            .decrypt::<TestClaims>(&download_token)
            .is_err());
        assert!(download_decryptor(&download_config)
            .decrypt::<TestClaims>(&upload_grant_token)
            .is_err());
        let download_wrong_implicit =
            local_token_with_implicit(&download_config, b"wrong-implicit");
        assert!(download_decryptor(&download_config)
            .decrypt::<TestClaims>(&download_wrong_implicit)
            .is_err());
    }

    #[test]
    fn rejects_bad_registered_claims_and_reserved_custom_claims() {
        let config = download_config("local-download");
        let encryptor = download_encryptor(&config);
        let expired = RegisteredClaims {
            expires_at: Utc::now() - ChronoDuration::seconds(1),
            ..registered()
        };
        assert!(encryptor.encrypt(expired, &custom_claims()).is_err());

        let custom = serde_json::json!({ "iss": "custom", "workspace_id": Uuid::new_v4() });
        assert!(encryptor.encrypt(registered(), &custom).is_err());

        let missing_exp = local_token_with_payload(
            &config,
            serde_json::json!({
                "iss": issuer().as_str(),
                "aud": DOWNLOAD_AUDIENCE,
                "sub": Uuid::new_v4(),
                "jti": Uuid::new_v4(),
                "iat": format_datetime(Utc::now()),
                "nbf": format_datetime(Utc::now()),
                "workspace_id": Uuid::new_v4(),
            }),
            DOWNLOAD_IMPLICIT_ASSERTION,
        );
        assert!(download_decryptor(&config)
            .decrypt::<TestClaims>(&missing_exp)
            .is_err());
    }

    fn local_token_with_implicit(
        config: &PasetoDownloadConfig,
        implicit_assertion: &[u8],
    ) -> String {
        let active = config
            .keys
            .iter()
            .find(|key| key.id == config.active_key_id)
            .unwrap();
        let secret = SymmetricKey::<V4>::try_from(active.secret.expose_secret()).unwrap();
        let footer = footer(&config.active_key_id).unwrap();
        let payload = payload(
            issuer().as_str(),
            DOWNLOAD_AUDIENCE,
            &registered(),
            &custom_claims(),
        )
        .unwrap();
        LocalToken::encrypt(
            &secret,
            payload.as_bytes(),
            Some(footer.as_bytes()),
            Some(implicit_assertion),
        )
        .unwrap()
    }

    fn local_token_with_payload(
        config: &PasetoDownloadConfig,
        payload: Value,
        implicit_assertion: &[u8],
    ) -> String {
        let active = config
            .keys
            .iter()
            .find(|key| key.id == config.active_key_id)
            .unwrap();
        let secret = SymmetricKey::<V4>::try_from(active.secret.expose_secret()).unwrap();
        let footer = footer(&config.active_key_id).unwrap();
        LocalToken::encrypt(
            &secret,
            serde_json::to_string(&payload).unwrap().as_bytes(),
            Some(footer.as_bytes()),
            Some(implicit_assertion),
        )
        .unwrap()
    }

    fn tamper(token: &str) -> String {
        let mut bytes = token.as_bytes().to_vec();
        let index = bytes.len() / 2;
        bytes[index] = if bytes[index] == b'A' { b'B' } else { b'A' };
        String::from_utf8(bytes).unwrap()
    }

    fn tamper_footer(token: &str) -> String {
        let (prefix, _) = token.rsplit_once('.').unwrap();
        format!("{prefix}.eyJraWQiOiJ1bmtub3duIn0")
    }
}
