use std::time::Duration;

use jwtk::{jwk::RemoteJwksVerifier, HeaderAndClaims};
use serde::de::DeserializeOwned;

const JWKS_CACHE_DURATION: Duration = Duration::from_secs(3600);

enum Backend {
    Remote(RemoteJwksVerifier),
    #[cfg(test)]
    Local(jwtk::jwk::JwkSetVerifier),
}

pub(crate) struct JwksVerifier {
    backend: Backend,
}

impl JwksVerifier {
    pub(crate) fn remote(jwks_url: impl Into<String>) -> Self {
        Self {
            backend: Backend::Remote(
                RemoteJwksVerifier::builder(jwks_url.into())
                    .with_cache_duration(JWKS_CACHE_DURATION)
                    .build(),
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn local(verifier: jwtk::jwk::JwkSetVerifier) -> Self {
        Self {
            backend: Backend::Local(verifier),
        }
    }

    pub(crate) async fn verify<ExtraClaims: DeserializeOwned>(
        &self,
        token: &str,
    ) -> jwtk::Result<HeaderAndClaims<ExtraClaims>> {
        match &self.backend {
            Backend::Remote(verifier) => verifier.verify(token).await,
            #[cfg(test)]
            Backend::Local(verifier) => verifier.verify(token),
        }
    }
}
