//! Resolving an MCP `client_id` to its declared metadata, regardless of which
//! identification scheme minted it.
//!
//! Two schemes coexist: clients that support CIMD publish an HTTPS metadata URL
//! as their `client_id` (`super::cimd`), while clients that rely on RFC 7591
//! dynamic registration receive a signed, self-describing `ppcli.` token
//! (`crate::authentication::client_registration`). This type owns both resolvers
//! and dispatches on the `client_id`'s format so callers deal with one resolve
//! (and one register) entry point and never branch on the scheme themselves.

use thiserror::Error;
use url::Url;

use crate::authentication::client_registration::{
    ClientIdResolution, ClientRegistrar, ClientRegistrationError, RegisterClientPayload,
    RegisteredClient,
};
use crate::authentication::paseto::Error as PasetoError;
use crate::config::PasetoMcpOAuthConfig;

use super::cimd::{CimdError, CimdResolver, ResolvedClient};

#[derive(Debug, Error)]
pub enum ClientResolutionError {
    #[error("client id metadata document could not be resolved")]
    Cimd(#[from] CimdError),
    #[error("signed client id could not be resolved")]
    Invalid(#[from] ClientRegistrationError),
    #[error("client id does not match a known format")]
    UnrecognizedFormat,
}

#[derive(Clone)]
pub struct ClientResolver {
    cimd: CimdResolver,
    registration: ClientRegistrar,
}

impl ClientResolver {
    /// Build both resolvers from the same `mcp_oauth` key material the rest of
    /// the MCP OAuth stack already uses.
    pub fn from_mcp_oauth_config(config: &PasetoMcpOAuthConfig) -> Result<Self, PasetoError> {
        Ok(Self {
            cimd: CimdResolver::new(),
            registration: ClientRegistrar::from_mcp_oauth_config(config)?,
        })
    }

    /// Mint a deterministic, signed `client_id` for a dynamically registering
    /// client (RFC 7591). Stateless.
    pub fn register(&self, payload: RegisterClientPayload) -> RegisteredClient {
        self.registration.register(payload)
    }

    /// Resolve a `client_id` to the client's declared metadata, dispatching on
    /// its format: a `ppcli.` token is verified offline, an HTTPS URL is fetched
    /// as a CIMD metadata document, and anything else is rejected without a
    /// network call.
    pub async fn resolve(&self, client_id: &str) -> Result<ResolvedClient, ClientResolutionError> {
        match self.registration.resolve_signed(client_id) {
            ClientIdResolution::Verified(client) => Ok(ResolvedClient {
                client_name: client.client_name,
                redirect_uris: client.redirect_uris,
            }),
            ClientIdResolution::Unrecognized if is_cimd_client_id(client_id) => {
                Ok(self.cimd.resolve(client_id).await?)
            }
            ClientIdResolution::Unrecognized => Err(ClientResolutionError::UnrecognizedFormat),
            ClientIdResolution::Invalid(error) => Err(ClientResolutionError::Invalid(error)),
        }
    }
}

fn is_cimd_client_id(client_id: &str) -> bool {
    Url::parse(client_id).is_ok_and(|url| url.scheme() == "https")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver() -> ClientResolver {
        // A single-key config is enough to exercise sign/verify dispatch; the
        // secret value is irrelevant to the format-routing behaviour under test.
        let config = PasetoMcpOAuthConfig {
            active_key_id: "test-key".to_owned(),
            keys: vec![crate::config::PasetoMcpOAuthKey {
                id: "test-key".to_owned(),
                secret: secrecy::SecretString::from("0123456789abcdef0123456789abcdef"),
            }],
        };
        ClientResolver::from_mcp_oauth_config(&config).expect("resolver builds")
    }

    fn register_payload() -> RegisterClientPayload {
        RegisterClientPayload {
            client_name: "Codex CLI".to_owned(),
            redirect_uris: vec!["http://localhost:1455/callback".to_owned()],
        }
    }

    #[tokio::test]
    async fn signed_client_id_resolves_offline() {
        let resolver = resolver();
        let registered = resolver.register(register_payload());

        let resolved = resolver
            .resolve(&registered.client_id)
            .await
            .expect("signed id resolves");
        assert_eq!(resolved.client_name, "Codex CLI");
        assert_eq!(resolved.redirect_uris, ["http://localhost/callback"]);
    }

    #[tokio::test]
    async fn tampered_signed_client_id_is_invalid() {
        let resolver = resolver();
        let id = resolver.register(register_payload()).client_id;
        // Corrupt the meta segment so the signature no longer matches.
        let parts: Vec<&str> = id.split('.').collect();
        let tampered = format!("ppcli.v1.{}.{}.{}", parts[2], "dGFtcGVy", parts[4]);

        assert!(matches!(
            resolver.resolve(&tampered).await,
            Err(ClientResolutionError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn https_url_client_id_is_routed_to_cimd() {
        let resolver = resolver();
        // An HTTPS-URL id must be handed to CIMD; a forbidden loopback host proves
        // the dispatch reached CIMD (and its SSRF guard) rather than the
        // signed-token path.
        assert!(matches!(
            resolver.resolve("https://127.0.0.1/metadata.json").await,
            Err(ClientResolutionError::Cimd(_))
        ));
    }

    #[tokio::test]
    async fn client_id_matching_no_scheme_is_rejected_without_fetch() {
        let resolver = resolver();
        // Neither a `ppcli.` token nor an HTTPS URL: reject outright, never CIMD.
        for client_id in ["ppoc_legacyrandomid", "http://localhost/cb", "not a url"] {
            assert!(
                matches!(
                    resolver.resolve(client_id).await,
                    Err(ClientResolutionError::UnrecognizedFormat)
                ),
                "expected {client_id} to be rejected as an unknown format"
            );
        }
    }
}
