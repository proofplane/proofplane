use std::time::{Duration, SystemTime};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, Utc};
use jwtk::{
    hmac::{HmacAlgorithm, HmacKey},
    sign, verify, HeaderAndClaims,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{de::DeserializeOwned, Serialize};
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IssuedToken {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedToken<T> {
    pub token: String,
    pub token_id: Uuid,
    pub claims: T,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub(crate) struct SignedJwt {
    issuer: Url,
    audience: String,
    ttl: Duration,
    key: HmacKey,
}

impl SignedJwt {
    pub(crate) fn new(
        issuer: Url,
        audience: impl Into<String>,
        ttl: Duration,
        signing_secret: &SecretString,
    ) -> Self {
        let key = BASE64_STANDARD
            .decode(signing_secret.expose_secret())
            .expect("JWT signing secret is validated during configuration loading");
        Self {
            issuer,
            audience: audience.into(),
            ttl,
            key: HmacKey::from_bytes(&key, HmacAlgorithm::HS256),
        }
    }

    pub(crate) fn issuer(&self) -> &Url {
        &self.issuer
    }

    pub(crate) fn issue<T: Serialize>(&self, claims: T) -> Result<IssuedToken, SignError> {
        let issued_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| SignError::Clock)?;
        let issued_at = Duration::from_secs(issued_at.as_secs());
        let expires = issued_at + self.ttl;
        let expires_at = duration_to_datetime(expires).ok_or(SignError::InvalidExpiration)?;
        let mut claims = HeaderAndClaims::with_claims(claims);
        claims.claims_mut().iat = Some(issued_at);
        claims.claims_mut().exp = Some(expires);
        claims
            .set_iss(self.issuer.as_str())
            .add_aud(&self.audience)
            .set_jti(Uuid::new_v4().to_string());

        Ok(IssuedToken {
            token: sign(&mut claims, &self.key).map_err(SignError::Signing)?,
            expires_at,
        })
    }

    pub(crate) fn verify<T: DeserializeOwned>(
        &self,
        token: &str,
    ) -> Result<VerifiedToken<T>, VerifyError> {
        let verified = verify::<serde_json::Value>(token, &self.key).map_err(|_| VerifyError)?;
        let claims = verified.claims();

        if claims.iss.as_deref() != Some(self.issuer.as_str())
            || !claims.aud.iter().any(|audience| audience == &self.audience)
        {
            return Err(VerifyError);
        }

        let issued_at = claims.iat.ok_or(VerifyError)?;
        let expires = claims.exp.ok_or(VerifyError)?;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| VerifyError)?;
        if issued_at > now
            || issued_at >= expires
            || expires - issued_at > self.ttl
            || duration_to_datetime(issued_at).is_none()
        {
            return Err(VerifyError);
        }

        Ok(VerifiedToken {
            token: token.to_owned(),
            token_id: parse_uuid(claims.jti.as_deref())?,
            claims: serde_json::from_value(claims.extra.clone()).map_err(|_| VerifyError)?,
            expires_at: duration_to_datetime(expires).ok_or(VerifyError)?,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SignError {
    #[error("system time is before the Unix epoch")]
    Clock,
    #[error("signed JWT expiration is outside the supported timestamp range")]
    InvalidExpiration,
    #[error("signed JWT signing failed")]
    Signing(#[source] jwtk::Error),
}

#[derive(Debug)]
pub(crate) struct VerifyError;

fn parse_uuid(value: Option<&str>) -> Result<Uuid, VerifyError> {
    value
        .filter(|value| !value.is_empty())
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or(VerifyError)
}

fn duration_to_datetime(value: Duration) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(i64::try_from(value.as_secs()).ok()?, value.subsec_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    const AUDIENCE: &str = "proofplane-test-token";
    const TTL: Duration = Duration::from_secs(90);

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestClaims {
        subject_id: String,
        permission: String,
    }

    fn jwt() -> SignedJwt {
        SignedJwt::new(
            Url::parse("https://api.proofplane.test/").unwrap(),
            AUDIENCE,
            TTL,
            &SecretString::from("MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY="),
        )
    }

    fn extra_claims() -> TestClaims {
        TestClaims {
            subject_id: "subject-123".to_owned(),
            permission: "read".to_owned(),
        }
    }

    fn claims() -> HeaderAndClaims<TestClaims> {
        let mut claims = HeaderAndClaims::with_claims(extra_claims());
        let issued_at = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();
        let issued_at = Duration::from_secs(issued_at.as_secs());
        claims.claims_mut().iat = Some(issued_at);
        claims.claims_mut().exp = Some(issued_at + TTL);
        claims
            .set_iss("https://api.proofplane.test/")
            .add_aud(AUDIENCE)
            .set_jti(Uuid::new_v4().to_string());
        claims
    }

    fn signed(jwt: &SignedJwt, mut claims: HeaderAndClaims<TestClaims>) -> String {
        sign(&mut claims, &jwt.key).unwrap()
    }

    #[test]
    fn issues_and_verifies_configured_hs256_token() {
        let jwt = jwt();
        let issued = jwt.issue(extra_claims()).unwrap();
        let verified = jwt.verify::<TestClaims>(&issued.token).unwrap();

        assert_eq!(verified.token, issued.token);
        assert_eq!(verified.claims, extra_claims());
        assert_eq!(verified.expires_at, issued.expires_at);
        assert_ne!(verified.token_id, Uuid::nil());
        assert!(issued.expires_at > Utc::now() + chrono::Duration::seconds(89));
        assert!(issued.expires_at <= Utc::now() + chrono::Duration::seconds(90));
    }

    #[test]
    fn rejects_tampering_wrong_algorithm_and_wrong_key() {
        let jwt = jwt();
        let issued = jwt.issue(extra_claims()).unwrap();
        let mut tampered = issued.token.into_bytes();
        let signature_byte = tampered.len() - 2;
        tampered[signature_byte] = if tampered[signature_byte] == b'A' {
            b'B'
        } else {
            b'A'
        };
        assert!(jwt
            .verify::<TestClaims>(std::str::from_utf8(&tampered).unwrap())
            .is_err());

        let wrong_algorithm =
            HmacKey::from_bytes(b"01234567890123456789012345678901", HmacAlgorithm::HS384);
        let token = sign(&mut claims(), &wrong_algorithm).unwrap();
        assert!(jwt.verify::<TestClaims>(&token).is_err());

        let wrong_key =
            HmacKey::from_bytes(b"abcdef0123456789abcdef0123456789", HmacAlgorithm::HS256);
        let token = sign(&mut claims(), &wrong_key).unwrap();
        assert!(jwt.verify::<TestClaims>(&token).is_err());
    }

    #[test]
    fn rejects_wrong_issuer_audience_and_missing_registered_claims() {
        let jwt = jwt();

        let mut wrong_issuer = claims();
        wrong_issuer.set_iss("https://wrong.example/");
        assert!(jwt
            .verify::<TestClaims>(&signed(&jwt, wrong_issuer))
            .is_err());

        let mut wrong_audience = claims();
        wrong_audience.claims_mut().aud = Default::default();
        wrong_audience.add_aud("wrong-audience");
        assert!(jwt
            .verify::<TestClaims>(&signed(&jwt, wrong_audience))
            .is_err());

        let mut missing_jti = claims();
        missing_jti.claims_mut().jti = None;
        assert!(jwt
            .verify::<TestClaims>(&signed(&jwt, missing_jti))
            .is_err());

        let mut missing_iat = claims();
        missing_iat.claims_mut().iat = None;
        assert!(jwt
            .verify::<TestClaims>(&signed(&jwt, missing_iat))
            .is_err());

        let mut missing_exp = claims();
        missing_exp.claims_mut().exp = None;
        assert!(jwt
            .verify::<TestClaims>(&signed(&jwt, missing_exp))
            .is_err());
    }

    #[test]
    fn rejects_invalid_lifetimes_and_expired_tokens() {
        let jwt = jwt();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap();

        let mut future = claims();
        future.claims_mut().iat = Some(now + Duration::from_secs(60));
        future.claims_mut().exp = Some(now + TTL);
        assert!(jwt.verify::<TestClaims>(&signed(&jwt, future)).is_err());

        let mut reversed = claims();
        reversed.claims_mut().iat = Some(now);
        reversed.claims_mut().exp = Some(now);
        assert!(jwt.verify::<TestClaims>(&signed(&jwt, reversed)).is_err());

        let mut too_long = claims();
        too_long.claims_mut().iat = Some(now);
        too_long.claims_mut().exp = Some(now + TTL + Duration::from_secs(2));
        assert!(jwt.verify::<TestClaims>(&signed(&jwt, too_long)).is_err());

        let mut expired = claims();
        expired.claims_mut().iat = Some(now - Duration::from_secs(180));
        expired.claims_mut().exp = Some(now - Duration::from_secs(90));
        assert!(jwt.verify::<TestClaims>(&signed(&jwt, expired)).is_err());
    }
}
