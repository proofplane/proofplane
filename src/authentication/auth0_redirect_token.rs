use std::time::Duration;

use jwtk::{
    hmac::{HmacAlgorithm, HmacKey},
    sign, verify_only, Claims, HeaderAndClaims, OneOrMany,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;
use url::Url;

pub const TOKEN_VERSION: u8 = 1;
pub const INPUT_PURPOSE: &str = "proofplane_agent_connection_consent";
pub const RESULT_PURPOSE: &str = "proofplane_agent_connection_result";
pub const MAX_TOKEN_LIFETIME_SECONDS: i64 = 300;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransactionExtraClaims {
    purpose: String,
    version: u8,
    transaction_id: String,
    oauth_state: String,
    client_id: String,
    client_name: String,
    resource: String,
    scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResultExtraClaims {
    purpose: String,
    version: u8,
    decision: ConsentDecision,
    transaction_id: String,
    oauth_state: String,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    continuation_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<String>,
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
        let verified = self.decode::<TransactionExtraClaims>(token)?;
        let claims = project_transaction_claims(verified.claims())?;
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
        claims: ConsentTransactionClaims,
    ) -> Result<String, RedirectTokenError> {
        let mut jwt = transaction_jwt(
            claims,
            self.auth0_token_issuer.clone(),
            self.consent_issuer.clone(),
        )?;
        self.encode(&mut jwt)
    }

    pub fn sign_result(&self, claims: ConsentResultClaims) -> Result<String, RedirectTokenError> {
        let mut jwt = result_jwt(
            claims,
            self.consent_issuer.clone(),
            self.auth0_issuer.clone(),
        )?;
        self.encode(&mut jwt)
    }

    pub fn verify_result(
        &self,
        token: &str,
        now: i64,
    ) -> Result<ConsentResultClaims, RedirectTokenError> {
        let verified = self.decode::<ResultExtraClaims>(token)?;
        let claims = project_result_claims(verified.claims())?;
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

    fn encode<T: Serialize>(
        &self,
        claims: &mut HeaderAndClaims<T>,
    ) -> Result<String, RedirectTokenError> {
        claims.header_mut().typ = Some("JWT".to_owned());
        sign(claims, &self.signing_key()).map_err(|_| RedirectTokenError::Signing)
    }

    fn decode<T: DeserializeOwned>(
        &self,
        token: &str,
    ) -> Result<HeaderAndClaims<T>, RedirectTokenError> {
        verify_only(token, &self.signing_key()).map_err(|_| RedirectTokenError::Invalid)
    }

    fn signing_key(&self) -> HmacKey {
        HmacKey::from_bytes(self.secret.expose_secret().as_bytes(), HmacAlgorithm::HS256)
    }
}

fn transaction_jwt(
    claims: ConsentTransactionClaims,
    issuer: String,
    audience: String,
) -> Result<HeaderAndClaims<TransactionExtraClaims>, RedirectTokenError> {
    let mut jwt = HeaderAndClaims::with_claims(TransactionExtraClaims {
        purpose: INPUT_PURPOSE.to_owned(),
        version: TOKEN_VERSION,
        transaction_id: claims.transaction_id,
        oauth_state: claims.oauth_state,
        client_id: claims.client_id,
        client_name: claims.client_name,
        resource: claims.resource,
        scopes: claims.scopes,
    });
    set_registered_claims(
        &mut jwt, issuer, audience, claims.sub, claims.iat, claims.exp,
    )?;
    Ok(jwt)
}

fn result_jwt(
    claims: ConsentResultClaims,
    issuer: String,
    audience: String,
) -> Result<HeaderAndClaims<ResultExtraClaims>, RedirectTokenError> {
    let mut jwt = HeaderAndClaims::with_claims(ResultExtraClaims {
        purpose: RESULT_PURPOSE.to_owned(),
        version: TOKEN_VERSION,
        decision: claims.decision,
        transaction_id: claims.transaction_id,
        oauth_state: claims.oauth_state,
        state: claims.state,
        continuation_token: claims.continuation_token,
        nonce: claims.nonce,
    });
    set_registered_claims(
        &mut jwt, issuer, audience, claims.sub, claims.iat, claims.exp,
    )?;
    Ok(jwt)
}

fn set_registered_claims<T>(
    jwt: &mut HeaderAndClaims<T>,
    issuer: String,
    audience: String,
    subject: String,
    issued_at: i64,
    expires_at: i64,
) -> Result<(), RedirectTokenError> {
    let issued_at = u64::try_from(issued_at).map_err(|_| RedirectTokenError::Signing)?;
    let expires_at = u64::try_from(expires_at).map_err(|_| RedirectTokenError::Signing)?;
    let registered = jwt.claims_mut();
    registered.iss = Some(issuer);
    registered.aud = OneOrMany::One(audience);
    registered.sub = Some(subject);
    registered.iat = Some(Duration::from_secs(issued_at));
    registered.exp = Some(Duration::from_secs(expires_at));
    Ok(())
}

fn project_transaction_claims(
    claims: &Claims<TransactionExtraClaims>,
) -> Result<ConsentTransactionClaims, RedirectTokenError> {
    let (iss, aud, sub, iat, exp) = project_registered_claims(claims)?;
    Ok(ConsentTransactionClaims {
        purpose: claims.extra.purpose.clone(),
        version: claims.extra.version,
        transaction_id: claims.extra.transaction_id.clone(),
        oauth_state: claims.extra.oauth_state.clone(),
        client_id: claims.extra.client_id.clone(),
        client_name: claims.extra.client_name.clone(),
        resource: claims.extra.resource.clone(),
        scopes: claims.extra.scopes.clone(),
        sub,
        iss,
        aud,
        iat,
        exp,
    })
}

fn project_result_claims(
    claims: &Claims<ResultExtraClaims>,
) -> Result<ConsentResultClaims, RedirectTokenError> {
    let (iss, aud, sub, iat, exp) = project_registered_claims(claims)?;
    Ok(ConsentResultClaims {
        purpose: claims.extra.purpose.clone(),
        version: claims.extra.version,
        decision: claims.extra.decision,
        sub,
        transaction_id: claims.extra.transaction_id.clone(),
        oauth_state: claims.extra.oauth_state.clone(),
        state: claims.extra.state.clone(),
        continuation_token: claims.extra.continuation_token.clone(),
        nonce: claims.extra.nonce.clone(),
        iss,
        aud,
        iat,
        exp,
    })
}

fn project_registered_claims<T>(
    claims: &Claims<T>,
) -> Result<(String, String, String, i64, i64), RedirectTokenError> {
    let issuer = claims.iss.clone().ok_or(RedirectTokenError::Invalid)?;
    let audience = match &claims.aud {
        OneOrMany::One(audience) => audience.clone(),
        OneOrMany::Vec(_) => return Err(RedirectTokenError::Invalid),
    };
    let subject = claims.sub.clone().ok_or(RedirectTokenError::Invalid)?;
    let issued_at = numeric_date(claims.iat)?;
    let expires_at = numeric_date(claims.exp)?;
    Ok((issuer, audience, subject, issued_at, expires_at))
}

fn numeric_date(value: Option<Duration>) -> Result<i64, RedirectTokenError> {
    let value = value.ok_or(RedirectTokenError::Invalid)?;
    if value.subsec_nanos() != 0 {
        return Err(RedirectTokenError::Invalid);
    }
    i64::try_from(value.as_secs()).map_err(|_| RedirectTokenError::Invalid)
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
    use jwtk::decode_without_verify;

    const NOW: i64 = 1_800_000_000;
    const AUTH0_TRANSACTION_TOKEN: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJwdXJwb3NlIjoicHJvb2ZwbGFuZV9hZ2VudF9jb25uZWN0aW9uX2NvbnNlbnQiLCJ2ZXJzaW9uIjoxLCJ0cmFuc2FjdGlvbl9pZCI6InRyYW5zYWN0aW9uIiwib2F1dGhfc3RhdGUiOiJvYXV0aC1zdGF0ZSIsImNsaWVudF9pZCI6ImNsaWVudCIsImNsaWVudF9uYW1lIjoiQ2xpZW50IiwicmVzb3VyY2UiOiJodHRwczovL21jcC5wcm9vZnBsYW5lLmNvbS9tY3AiLCJzY29wZXMiOlsicmVhZF9jb250cm9scyJdLCJzdWIiOiJhdXRoMHx1c2VyIiwiaXNzIjoidGVuYW50LmF1dGgwLmNvbSIsImF1ZCI6Imh0dHBzOi8vYXBpLnByb29mcGxhbmUuY29tL2FnZW50LWNvbm5lY3Rpb25zL2NvbnNlbnQiLCJpYXQiOjE4MDAwMDAwMDAsImV4cCI6MTgwMDAwMDMwMH0.9D0wNJ2kA68RAn4FHXyH1giYrB3T3sxtzdUsh25VjKU";

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

    #[test]
    fn independently_generated_auth0_transaction_token_verifies() {
        let claims = codec()
            .verify_transaction(AUTH0_TRANSACTION_TOKEN, NOW)
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
        let decoded = decode_without_verify::<ResultExtraClaims>(&token).expect("token decodes");
        assert_eq!(decoded.header().alg, "HS256");
        assert_eq!(decoded.header().typ.as_deref(), Some("JWT"));
        assert_eq!(
            decoded.claims().aud,
            OneOrMany::One("https://tenant.auth0.com/".to_owned())
        );

        let claims = codec().verify_result(&token, NOW).expect("token verifies");
        assert_eq!(claims.purpose, RESULT_PURPOSE);
        assert_eq!(claims.version, TOKEN_VERSION);
        assert_eq!(claims.aud, "https://tenant.auth0.com/");
    }

    #[test]
    fn tampering_and_wrong_algorithm_are_rejected() {
        let token = codec().sign_result(result_claims()).unwrap();
        let (signed_data, signature) = token.rsplit_once('.').unwrap();
        let mut signature = signature.to_owned();
        let replacement = if signature.starts_with('A') { "B" } else { "A" };
        signature.replace_range(..1, replacement);
        let tampered = format!("{signed_data}.{signature}");
        assert_eq!(
            codec().verify_result(&tampered, NOW),
            Err(RedirectTokenError::Invalid)
        );

        let mut wrong_algorithm = result_jwt(
            result_claims(),
            codec().consent_issuer.clone(),
            codec().auth0_issuer.clone(),
        )
        .unwrap();
        wrong_algorithm.header_mut().typ = Some("JWT".to_owned());
        let key = HmacKey::from_bytes(
            codec().secret.expose_secret().as_bytes(),
            HmacAlgorithm::HS384,
        );
        let wrong_algorithm = sign(&mut wrong_algorithm, &key).unwrap();
        assert_eq!(
            codec().verify_result(&wrong_algorithm, NOW),
            Err(RedirectTokenError::Invalid)
        );
    }

    #[test]
    fn issuer_audience_purpose_lifetime_and_expiry_are_enforced() {
        for mutate in [
            |jwt: &mut HeaderAndClaims<ResultExtraClaims>| {
                jwt.claims_mut().iss = Some("wrong".to_owned())
            },
            |jwt: &mut HeaderAndClaims<ResultExtraClaims>| {
                jwt.claims_mut().aud = OneOrMany::One("wrong".to_owned())
            },
            |jwt: &mut HeaderAndClaims<ResultExtraClaims>| {
                jwt.claims_mut().extra.purpose = "wrong".to_owned()
            },
            |jwt: &mut HeaderAndClaims<ResultExtraClaims>| jwt.claims_mut().extra.version = 2,
        ] {
            let mut jwt = result_jwt(
                result_claims(),
                codec().consent_issuer.clone(),
                codec().auth0_issuer.clone(),
            )
            .unwrap();
            mutate(&mut jwt);
            let token = codec().encode(&mut jwt).unwrap();
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

        for (iat, exp) in [(NOW + 1, NOW + 300), (NOW, NOW)] {
            let mut invalid_order = result_claims();
            invalid_order.iat = iat;
            invalid_order.exp = exp;
            let token = codec().sign_result(invalid_order).unwrap();
            assert_eq!(
                codec().verify_result(&token, NOW),
                Err(RedirectTokenError::Invalid)
            );
        }
    }

    #[test]
    fn audience_must_be_a_single_string_and_subject_must_be_present() {
        for mutate in [
            |jwt: &mut HeaderAndClaims<ResultExtraClaims>| {
                jwt.claims_mut().aud = OneOrMany::Vec(vec!["https://tenant.auth0.com/".to_owned()])
            },
            |jwt: &mut HeaderAndClaims<ResultExtraClaims>| jwt.claims_mut().sub = None,
        ] {
            let mut jwt = result_jwt(
                result_claims(),
                codec().consent_issuer.clone(),
                codec().auth0_issuer.clone(),
            )
            .unwrap();
            mutate(&mut jwt);
            let token = codec().encode(&mut jwt).unwrap();
            assert_eq!(
                codec().verify_result(&token, NOW),
                Err(RedirectTokenError::Invalid)
            );
        }
    }

    #[test]
    fn result_state_and_decision_secret_shape_are_enforced() {
        let mut blank_state = result_claims();
        blank_state.state = " ".to_owned();

        let mut approved_without_nonce = result_claims();
        approved_without_nonce.nonce = None;

        let mut denied_with_secrets = result_claims();
        denied_with_secrets.decision = ConsentDecision::Denied;

        for claims in [blank_state, approved_without_nonce, denied_with_secrets] {
            let token = codec().sign_result(claims).unwrap();
            assert_eq!(
                codec().verify_result(&token, NOW),
                Err(RedirectTokenError::Invalid)
            );
        }

        let mut denied = result_claims();
        denied.decision = ConsentDecision::Denied;
        denied.continuation_token = None;
        denied.nonce = None;
        let token = codec().sign_result(denied).unwrap();
        assert_eq!(
            codec().verify_result(&token, NOW).unwrap().decision,
            ConsentDecision::Denied
        );
    }
}
