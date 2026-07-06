use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretString};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use url::Url;

pub const TOKEN_VERSION: u8 = 1;
pub const INPUT_PURPOSE: &str = "proofplane_agent_connection_consent";
pub const RESULT_PURPOSE: &str = "proofplane_agent_connection_result";
pub const MAX_TOKEN_LIFETIME_SECONDS: i64 = 300;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsentTransactionClaims {
    pub purpose: String,
    pub version: u8,
    pub transaction_id: String,
    pub oauth_state: String,
    pub client_id: String,
    pub client_name: String,
    pub resource: String,
    pub scopes: Vec<String>,
    pub sub: String,
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsentResultClaims {
    pub purpose: String,
    pub version: u8,
    pub decision: ConsentDecision,
    pub sub: String,
    pub transaction_id: String,
    pub oauth_state: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsentDecision {
    Approved,
    Denied,
}

#[derive(Debug, Clone)]
pub struct RedirectTokenCodec {
    secret: SecretString,
    auth0_issuer: String,
    auth0_token_issuer: String,
    consent_issuer: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RedirectTokenError {
    #[error("invalid redirect token")]
    Invalid,
    #[error("expired redirect token")]
    Expired,
    #[error("redirect token lifetime exceeds the allowed maximum")]
    ExcessiveLifetime,
    #[error("redirect token signing failed")]
    Signing,
}

impl RedirectTokenCodec {
    pub fn new(
        secret: SecretString,
        auth0_issuer: impl Into<String>,
        consent_issuer: impl Into<String>,
    ) -> Self {
        let auth0_issuer = auth0_issuer.into();
        let auth0_token_issuer = Url::parse(&auth0_issuer)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| auth0_issuer.clone());
        Self {
            secret,
            auth0_issuer,
            auth0_token_issuer,
            consent_issuer: consent_issuer.into(),
        }
    }

    pub fn verify_transaction(
        &self,
        token: &str,
        now: i64,
    ) -> Result<ConsentTransactionClaims, RedirectTokenError> {
        let claims: ConsentTransactionClaims = self.decode(token)?;
        validate_registered_claims(
            &claims.iss,
            &claims.aud,
            &claims.purpose,
            claims.version,
            claims.iat,
            claims.exp,
            &self.auth0_token_issuer,
            &self.consent_issuer,
            INPUT_PURPOSE,
            now,
        )?;
        Ok(claims)
    }

    pub fn sign_transaction(
        &self,
        mut claims: ConsentTransactionClaims,
    ) -> Result<String, RedirectTokenError> {
        claims.purpose = INPUT_PURPOSE.to_owned();
        claims.version = TOKEN_VERSION;
        claims.iss = self.auth0_token_issuer.clone();
        claims.aud = self.consent_issuer.clone();
        self.encode(&claims)
    }

    pub fn sign_result(
        &self,
        mut claims: ConsentResultClaims,
    ) -> Result<String, RedirectTokenError> {
        claims.purpose = RESULT_PURPOSE.to_owned();
        claims.version = TOKEN_VERSION;
        claims.iss = self.consent_issuer.clone();
        claims.aud = self.auth0_issuer.clone();
        self.encode(&claims)
    }

    pub fn verify_result(
        &self,
        token: &str,
        now: i64,
    ) -> Result<ConsentResultClaims, RedirectTokenError> {
        let claims: ConsentResultClaims = self.decode(token)?;
        validate_registered_claims(
            &claims.iss,
            &claims.aud,
            &claims.purpose,
            claims.version,
            claims.iat,
            claims.exp,
            &self.consent_issuer,
            &self.auth0_issuer,
            RESULT_PURPOSE,
            now,
        )?;
        if !not_blank(&claims.state) {
            return Err(RedirectTokenError::Invalid);
        }
        let secret_shape_is_valid = match claims.decision {
            ConsentDecision::Approved => {
                claims.continuation_token.as_deref().is_some_and(not_blank)
                    && claims.nonce.as_deref().is_some_and(not_blank)
            }
            ConsentDecision::Denied => {
                claims.continuation_token.is_none() && claims.nonce.is_none()
            }
        };
        if !secret_shape_is_valid {
            return Err(RedirectTokenError::Invalid);
        }
        Ok(claims)
    }

    fn encode<T: Serialize>(&self, claims: &T) -> Result<String, RedirectTokenError> {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let payload = serde_json::to_vec(claims).map_err(|_| RedirectTokenError::Signing)?;
        let payload = URL_SAFE_NO_PAD.encode(payload);
        let signing_input = format!("{header}.{payload}");
        let mut mac = HmacSha256::new_from_slice(self.secret.expose_secret().as_bytes())
            .map_err(|_| RedirectTokenError::Signing)?;
        mac.update(signing_input.as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        Ok(format!("{signing_input}.{signature}"))
    }

    fn decode<T: DeserializeOwned>(&self, token: &str) -> Result<T, RedirectTokenError> {
        let mut segments = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) = (
            segments.next(),
            segments.next(),
            segments.next(),
            segments.next(),
        ) else {
            return Err(RedirectTokenError::Invalid);
        };
        let header_value: serde_json::Value = decode_json(header)?;
        if header_value.get("alg").and_then(|value| value.as_str()) != Some("HS256") {
            return Err(RedirectTokenError::Invalid);
        }
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| RedirectTokenError::Invalid)?;
        let mut mac = HmacSha256::new_from_slice(self.secret.expose_secret().as_bytes())
            .map_err(|_| RedirectTokenError::Invalid)?;
        mac.update(format!("{header}.{payload}").as_bytes());
        mac.verify_slice(&signature)
            .map_err(|_| RedirectTokenError::Invalid)?;
        decode_json(payload)
    }
}

fn decode_json<T: DeserializeOwned>(segment: &str) -> Result<T, RedirectTokenError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| RedirectTokenError::Invalid)?;
    serde_json::from_slice(&bytes).map_err(|_| RedirectTokenError::Invalid)
}

#[allow(clippy::too_many_arguments)]
fn validate_registered_claims(
    issuer: &str,
    audience: &str,
    purpose: &str,
    version: u8,
    issued_at: i64,
    expires_at: i64,
    expected_issuer: &str,
    expected_audience: &str,
    expected_purpose: &str,
    now: i64,
) -> Result<(), RedirectTokenError> {
    if issuer != expected_issuer
        || audience != expected_audience
        || purpose != expected_purpose
        || version != TOKEN_VERSION
        || issued_at > now
        || expires_at <= issued_at
    {
        return Err(RedirectTokenError::Invalid);
    }
    if expires_at - issued_at > MAX_TOKEN_LIFETIME_SECONDS {
        return Err(RedirectTokenError::ExcessiveLifetime);
    }
    if expires_at <= now {
        return Err(RedirectTokenError::Expired);
    }
    Ok(())
}

fn not_blank(value: &str) -> bool {
    !value.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn codec() -> RedirectTokenCodec {
        RedirectTokenCodec::new(
            SecretString::from("01234567890123456789012345678901"),
            "https://tenant.auth0.com/",
            "https://api.proofplane.com/agent-connections/consent",
        )
    }

    fn result_claims() -> ConsentResultClaims {
        ConsentResultClaims {
            purpose: String::new(),
            version: 0,
            decision: ConsentDecision::Approved,
            sub: "auth0|user".to_owned(),
            transaction_id: "transaction".to_owned(),
            oauth_state: "oauth-state".to_owned(),
            state: "auth0-redirect-state".to_owned(),
            continuation_token: Some("continuation".to_owned()),
            nonce: Some("nonce".to_owned()),
            iss: String::new(),
            aud: String::new(),
            iat: NOW,
            exp: NOW + 300,
        }
    }

    fn transaction_claims() -> ConsentTransactionClaims {
        ConsentTransactionClaims {
            purpose: String::new(),
            version: 0,
            transaction_id: "transaction".to_owned(),
            oauth_state: "oauth-state".to_owned(),
            client_id: "client".to_owned(),
            client_name: "Client".to_owned(),
            resource: "https://mcp.proofplane.com/mcp".to_owned(),
            scopes: vec!["read_controls".to_owned()],
            sub: "auth0|user".to_owned(),
            iss: String::new(),
            aud: String::new(),
            iat: NOW,
            exp: NOW + 300,
        }
    }

    #[test]
    fn auth0_native_transaction_claims_round_trip() {
        let token = codec()
            .sign_transaction(transaction_claims())
            .expect("transaction signs");
        let claims = codec()
            .verify_transaction(&token, NOW)
            .expect("transaction verifies");
        assert_eq!(claims.iss, "tenant.auth0.com");
        assert_eq!(
            claims.aud,
            "https://api.proofplane.com/agent-connections/consent"
        );
        assert_eq!(claims.purpose, INPUT_PURPOSE);
        assert_eq!(claims.sub, "auth0|user");
    }

    #[test]
    fn auth0_compatible_hs256_result_round_trips_with_known_claims() {
        let token = codec().sign_result(result_claims()).expect("token signs");
        let header: serde_json::Value = decode_json(token.split('.').next().unwrap()).unwrap();
        assert_eq!(header["alg"], "HS256");
        assert_eq!(header["typ"], "JWT");

        let claims = codec().verify_result(&token, NOW).expect("token verifies");
        assert_eq!(claims.purpose, RESULT_PURPOSE);
        assert_eq!(claims.version, TOKEN_VERSION);
        assert_eq!(claims.aud, "https://tenant.auth0.com/");
    }

    #[test]
    fn tampering_and_wrong_algorithm_are_rejected() {
        let token = codec().sign_result(result_claims()).unwrap();
        let mut tampered = token.clone();
        let replacement = if tampered.ends_with('A') { "B" } else { "A" };
        tampered.replace_range(tampered.len() - 1.., replacement);
        assert_eq!(
            codec().verify_result(&tampered, NOW),
            Err(RedirectTokenError::Invalid)
        );

        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
        let rest = token.split_once('.').unwrap().1;
        assert_eq!(
            codec().verify_result(&format!("{header}.{rest}"), NOW),
            Err(RedirectTokenError::Invalid)
        );
    }

    #[test]
    fn issuer_audience_purpose_lifetime_and_expiry_are_enforced() {
        for mutate in [
            |claims: &mut ConsentResultClaims| claims.iss = "wrong".to_owned(),
            |claims: &mut ConsentResultClaims| claims.aud = "wrong".to_owned(),
            |claims: &mut ConsentResultClaims| claims.purpose = "wrong".to_owned(),
        ] {
            let mut claims = result_claims();
            claims.purpose = RESULT_PURPOSE.to_owned();
            claims.version = TOKEN_VERSION;
            claims.iss = codec().consent_issuer.clone();
            claims.aud = codec().auth0_issuer.clone();
            mutate(&mut claims);
            let token = codec().encode(&claims).unwrap();
            assert_eq!(
                codec().verify_result(&token, NOW),
                Err(RedirectTokenError::Invalid)
            );
        }

        let mut excessive = result_claims();
        excessive.exp = NOW + 301;
        let token = codec().sign_result(excessive).unwrap();
        assert_eq!(
            codec().verify_result(&token, NOW),
            Err(RedirectTokenError::ExcessiveLifetime)
        );

        let mut expired = result_claims();
        expired.iat = NOW - 300;
        expired.exp = NOW;
        let token = codec().sign_result(expired).unwrap();
        assert_eq!(
            codec().verify_result(&token, NOW),
            Err(RedirectTokenError::Expired)
        );
    }
}
