use std::collections::HashMap;

use chrono::{DateTime, SecondsFormat, Utc};
use pasetors::{
    keys::{AsymmetricPublicKey, AsymmetricSecretKey, SymmetricKey},
    token::{Local, Public, UntrustedToken},
    version4::{LocalToken, PublicToken, V4},
};
use secrecy::ExposeSecret;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{Map, Value};
use url::Url;
use uuid::Uuid;

use crate::config::{PasetoApiConfig, PasetoDownloadConfig};

// We use assertions as a mechanism for verifying that the API key is being
// used for the right purpose. For example, for API keys, we want to make sure
// that the tokens we're verifying were issued with the intention of them
// being API keys and not something else.
const API_IMPLICIT_ASSERTION: &[u8] = b"proofplane:api-access:v1";
const DOWNLOAD_IMPLICIT_ASSERTION: &[u8] = b"proofplane:attachment-download:v1";
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
pub struct ApiTokenSigner {
    issuer: Url,
    audience: String,
    key_id: String,
    secret_key: AsymmetricSecretKey<V4>,
}

impl ApiTokenSigner {
    pub fn from_config(
        issuer: Url,
        audience: impl Into<String>,
        config: &PasetoApiConfig,
    ) -> Result<Self, Error> {
        Ok(Self {
            issuer,
            audience: audience.into(),
            key_id: config.active_signing_key.id.clone(),
            secret_key: AsymmetricSecretKey::<V4>::try_from(
                config.active_signing_key.secret.expose_secret(),
            )
            .map_err(|_| Error::Keyring)?,
        })
    }

    pub fn issue<T: Serialize>(
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
        let token = PublicToken::sign(
            &self.secret_key,
            payload.as_bytes(),
            Some(footer.as_bytes()),
            Some(API_IMPLICIT_ASSERTION),
        )
        .map_err(|_| Error::Issue)?;

        Ok(IssuedPasetoToken {
            token,
            token_id: registered.token_id,
            key_id: self.key_id.clone(),
            expires_at: normalize_datetime(registered.expires_at)?,
        })
    }
}

#[derive(Clone)]
pub struct ApiTokenVerifier {
    issuer: Url,
    audience: String,
    verification_keys: HashMap<String, AsymmetricPublicKey<V4>>,
}

impl ApiTokenVerifier {
    pub fn from_config(
        issuer: Url,
        audience: impl Into<String>,
        config: &PasetoApiConfig,
    ) -> Result<Self, Error> {
        let mut verification_keys = HashMap::new();
        for key in &config.verification_keys {
            verification_keys.insert(
                key.id.clone(),
                AsymmetricPublicKey::<V4>::try_from(key.public.as_str())
                    .map_err(|_| Error::Keyring)?,
            );
        }

        Ok(Self {
            issuer,
            audience: audience.into(),
            verification_keys,
        })
    }

    pub fn verify<T: DeserializeOwned>(
        &self,
        token: &str,
    ) -> Result<VerifiedPasetoToken<T>, Error> {
        let untrusted = UntrustedToken::<Public, V4>::try_from(token).map_err(|_| Error::Verify)?;
        let footer = parse_footer(untrusted.untrusted_footer())?;
        let key = self
            .verification_keys
            .get(&footer.kid)
            .ok_or(Error::Verify)?;
        let trusted = PublicToken::verify(
            key,
            &untrusted,
            Some(untrusted.untrusted_footer()),
            Some(API_IMPLICIT_ASSERTION),
        )
        .map_err(|_| Error::Verify)?;

        verified_payload(
            trusted.payload(),
            &footer.kid,
            self.issuer.as_str(),
            &self.audience,
        )
    }
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
            secret_key: SymmetricKey::<V4>::try_from(active.secret.expose_secret())
                .map_err(|_| Error::Keyring)?,
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
        let token = LocalToken::encrypt(
            &self.secret_key,
            payload.as_bytes(),
            Some(footer.as_bytes()),
            Some(DOWNLOAD_IMPLICIT_ASSERTION),
        )
        .map_err(|_| Error::Issue)?;

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
            keys.insert(
                key.id.clone(),
                SymmetricKey::<V4>::try_from(key.secret.expose_secret())
                    .map_err(|_| Error::Keyring)?,
            );
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
        let trusted = LocalToken::decrypt(
            key,
            &untrusted,
            Some(untrusted.untrusted_footer()),
            Some(DOWNLOAD_IMPLICIT_ASSERTION),
        )
        .map_err(|_| Error::Verify)?;

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
    use crate::config::{PasetoApiSigningKey, PasetoApiVerificationKey, PasetoDownloadKey};

    const API_AUDIENCE: &str = "proofplane-api";
    const DOWNLOAD_AUDIENCE: &str = "proofplane-attachment-download";
    const API_SECRET: &str = "k4.secret.sEP9YtkNeO7EGJbpVYznvHnVXotZyGbkzuvHkOO3RgXAqGWIhrrfscm74zMx72tBOOD02gy8G4sB8-60b1cWiw";
    const API_PUBLIC: &str = "k4.public.wKhliIa637HJu-MzMe9rQTjg9NoMvBuLAfPutG9XFos";
    const OTHER_API_SECRET: &str = "k4.secret.9CgkMno7WtJFoVtui7YAo64QdHF4gEL9wrrTbsVvSRl0mFbeQ1nOzcYLVmdzOxpaKpPP-DYcTY9ETWV7ismu3g";
    const OTHER_API_PUBLIC: &str = "k4.public.dJhW3kNZzs3GC1ZnczsaWiqTz_g2HE2PRE1le4rJrt4";
    const DOWNLOAD_SECRET: &str = "k4.local.mKj2EzeLOuNBNlHNX6oLl76yopCc1K9YvWQVIo1xYEs";
    const OTHER_DOWNLOAD_SECRET: &str = "k4.local.cMO6bYZvmIk4f5OppaRjsRYQE0frbAM7qD4cDAO8HxY";

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

    fn api_config(active_id: &str) -> PasetoApiConfig {
        PasetoApiConfig {
            active_signing_key: PasetoApiSigningKey {
                id: active_id.to_owned(),
                secret: SecretString::from(if active_id == "old-api" {
                    OTHER_API_SECRET
                } else {
                    API_SECRET
                }),
            },
            verification_keys: vec![
                PasetoApiVerificationKey {
                    id: "local-api".to_owned(),
                    public: API_PUBLIC.to_owned(),
                },
                PasetoApiVerificationKey {
                    id: "old-api".to_owned(),
                    public: OTHER_API_PUBLIC.to_owned(),
                },
            ],
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

    fn api_signer(config: &PasetoApiConfig) -> ApiTokenSigner {
        ApiTokenSigner::from_config(issuer(), API_AUDIENCE, config).unwrap()
    }

    fn api_verifier(config: &PasetoApiConfig) -> ApiTokenVerifier {
        ApiTokenVerifier::from_config(issuer(), API_AUDIENCE, config).unwrap()
    }

    fn download_encryptor(config: &PasetoDownloadConfig) -> DownloadGrantEncryptor {
        DownloadGrantEncryptor::from_config(issuer(), DOWNLOAD_AUDIENCE, config).unwrap()
    }

    fn download_decryptor(config: &PasetoDownloadConfig) -> DownloadGrantDecryptor {
        DownloadGrantDecryptor::from_config(issuer(), DOWNLOAD_AUDIENCE, config).unwrap()
    }

    #[test]
    fn api_public_tokens_round_trip() {
        let config = api_config("local-api");
        let registered = registered();
        let claims = custom_claims();
        let issued = api_signer(&config)
            .issue(registered.clone(), &claims)
            .unwrap();
        let verified = api_verifier(&config)
            .verify::<TestClaims>(&issued.token)
            .unwrap();

        assert_eq!(issued.token_id, registered.token_id);
        assert_eq!(verified.subject, registered.subject);
        assert_eq!(verified.token_id, registered.token_id);
        assert_eq!(verified.key_id, "local-api");
        assert_eq!(verified.claims, claims);
        assert_eq!(verified.expires_at, issued.expires_at);
        assert!(issued.token.starts_with("v4.public."));
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
        let api_config = api_config("local-api");
        let issued = api_signer(&api_config)
            .issue(registered(), &custom_claims())
            .unwrap();

        assert!(api_verifier(&api_config)
            .verify::<TestClaims>("not-a-token")
            .is_err());

        let without_key = PasetoApiConfig {
            active_signing_key: api_config.active_signing_key.clone(),
            verification_keys: vec![],
        };
        assert!(api_verifier(&without_key)
            .verify::<TestClaims>(&issued.token)
            .is_err());

        let wrong_key = PasetoApiConfig {
            active_signing_key: api_config.active_signing_key.clone(),
            verification_keys: vec![PasetoApiVerificationKey {
                id: "local-api".to_owned(),
                public: OTHER_API_PUBLIC.to_owned(),
            }],
        };
        assert!(api_verifier(&wrong_key)
            .verify::<TestClaims>(&issued.token)
            .is_err());

        assert!(api_verifier(&api_config)
            .verify::<TestClaims>(&tamper(&issued.token))
            .is_err());
        assert!(api_verifier(&api_config)
            .verify::<TestClaims>(&tamper_footer(&issued.token))
            .is_err());

        let download_config = download_config("local-download");
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
        let old_api_config = api_config("old-api");
        let api_token = api_signer(&old_api_config)
            .issue(registered(), &custom_claims())
            .unwrap()
            .token;
        assert!(api_verifier(&api_config("local-api"))
            .verify::<TestClaims>(&api_token)
            .is_ok());
        let removed_old_api = PasetoApiConfig {
            active_signing_key: api_config("local-api").active_signing_key,
            verification_keys: vec![PasetoApiVerificationKey {
                id: "local-api".to_owned(),
                public: API_PUBLIC.to_owned(),
            }],
        };
        assert!(api_verifier(&removed_old_api)
            .verify::<TestClaims>(&api_token)
            .is_err());

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
        let api_config = api_config("local-api");
        let download_config = download_config("local-download");
        let api_token = api_signer(&api_config)
            .issue(registered(), &custom_claims())
            .unwrap()
            .token;
        let download_token = download_encryptor(&download_config)
            .encrypt(registered(), &custom_claims())
            .unwrap()
            .token;

        assert!(download_decryptor(&download_config)
            .decrypt::<TestClaims>(&api_token)
            .is_err());
        assert!(api_verifier(&api_config)
            .verify::<TestClaims>(&download_token)
            .is_err());

        let wrong_audience_verifier =
            ApiTokenVerifier::from_config(issuer(), "wrong-audience", &api_config).unwrap();
        assert!(wrong_audience_verifier
            .verify::<TestClaims>(&api_token)
            .is_err());

        let api_wrong_implicit = public_token_with_implicit(&api_config, b"wrong-implicit");
        assert!(api_verifier(&api_config)
            .verify::<TestClaims>(&api_wrong_implicit)
            .is_err());
        let download_wrong_implicit =
            local_token_with_implicit(&download_config, b"wrong-implicit");
        assert!(download_decryptor(&download_config)
            .decrypt::<TestClaims>(&download_wrong_implicit)
            .is_err());
    }

    #[test]
    fn rejects_bad_registered_claims_and_reserved_custom_claims() {
        let config = api_config("local-api");
        let signer = api_signer(&config);
        let expired = RegisteredClaims {
            expires_at: Utc::now() - ChronoDuration::seconds(1),
            ..registered()
        };
        assert!(signer.issue(expired, &custom_claims()).is_err());

        let custom = serde_json::json!({ "iss": "custom", "workspace_id": Uuid::new_v4() });
        assert!(signer.issue(registered(), &custom).is_err());

        let missing_exp = public_token_with_payload(
            &config,
            serde_json::json!({
                "iss": issuer().as_str(),
                "aud": API_AUDIENCE,
                "sub": Uuid::new_v4(),
                "jti": Uuid::new_v4(),
                "iat": format_datetime(Utc::now()),
                "nbf": format_datetime(Utc::now()),
                "workspace_id": Uuid::new_v4(),
            }),
            API_IMPLICIT_ASSERTION,
        );
        assert!(api_verifier(&config)
            .verify::<TestClaims>(&missing_exp)
            .is_err());
    }

    fn public_token_with_implicit(config: &PasetoApiConfig, implicit_assertion: &[u8]) -> String {
        public_token_with_payload(
            config,
            serde_json::from_str(
                &payload(
                    issuer().as_str(),
                    API_AUDIENCE,
                    &registered(),
                    &custom_claims(),
                )
                .unwrap(),
            )
            .unwrap(),
            implicit_assertion,
        )
    }

    fn public_token_with_payload(
        config: &PasetoApiConfig,
        payload: Value,
        implicit_assertion: &[u8],
    ) -> String {
        let secret =
            AsymmetricSecretKey::<V4>::try_from(config.active_signing_key.secret.expose_secret())
                .unwrap();
        let footer = footer(&config.active_signing_key.id).unwrap();
        PublicToken::sign(
            &secret,
            serde_json::to_string(&payload).unwrap().as_bytes(),
            Some(footer.as_bytes()),
            Some(implicit_assertion),
        )
        .unwrap()
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
