//! Stateless RFC 7591 Dynamic Client Registration.
//!
//! Some MCP clients (notably Codex) do not support CIMD: they unconditionally
//! POST to a `registration_endpoint` and expect a `client_id` back. Rather than
//! minting a random id and persisting an `oauth_clients` row — which would break
//! agent-connection dedup, because those clients re-register on every login and
//! reuse is keyed on the `client_id` string — the registrar mints a
//! **deterministic, signed, self-describing** `client_id`:
//!
//! ```text
//! ppcli.v1.<b64url(kid)>.<b64url(canonical_meta_json)>.<b64url(hmac_sha256(k, signing_input))>
//! ```
//!
//! The meta encodes `{client_name, redirect_uris}` with the redirect URIs
//! normalized, sorted, and deduped, so the same client always yields the same
//! id — dedup is preserved with zero server state. The HMAC (over everything
//! before the final segment) makes the id unforgeable, so an attacker cannot
//! craft an id declaring a redirect they control. This mirrors CIMD's trust
//! model: anyone may register, but consent + declared-redirect matching are the
//! real controls.

use std::collections::HashMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

use crate::authentication::paseto::Error as PasetoError;
use crate::config::PasetoMcpOAuthConfig;
use crate::services::cimd::ResolvedClient;

/// Fixed version prefix of every minted `client_id`.
const PREFIX: &str = "ppcli.v1";
/// Domain-separation label so the HMAC key is cryptographically independent of
/// the PASETO encryption use of the same configured secret.
const KEY_DERIVATION_LABEL: &[u8] = b"proofplane:client-registration:v1:";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Error)]
pub enum ClientRegistrationError {
    /// The id carried our prefix but was structurally invalid.
    #[error("client id is malformed")]
    Malformed,
    /// The id named a signing key we do not hold (e.g. rotated out).
    #[error("client id references an unknown signing key")]
    UnknownKey,
    /// The id's signature did not verify — tampered or forged.
    #[error("client id signature is invalid")]
    InvalidSignature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterClientPayload {
    pub client_name: String,
    pub redirect_uris: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredClient {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
}

/// The self-describing payload embedded in a `client_id`. Field order is fixed
/// and the redirect URIs are normalized/sorted/deduped before serialization, so
/// `serde_json` produces byte-identical output for identical logical clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CanonicalMeta {
    client_name: String,
    redirect_uris: Vec<String>,
}

#[derive(Clone)]
pub struct ClientRegistrar {
    keys: HashMap<String, [u8; 32]>,
    active_kid: String,
}

impl ClientRegistrar {
    /// Derive one HMAC key per configured `mcp_oauth` key. No new secret is
    /// introduced: each key is `SHA256(label || configured_secret)`.
    pub fn from_mcp_oauth_config(config: &PasetoMcpOAuthConfig) -> Result<Self, PasetoError> {
        let mut keys = HashMap::new();
        for key in &config.keys {
            keys.insert(key.id.clone(), derive_key(key.secret.expose_secret()));
        }
        if !keys.contains_key(&config.active_key_id) {
            return Err(PasetoError::Keyring);
        }
        Ok(Self {
            keys,
            active_kid: config.active_key_id.clone(),
        })
    }

    /// Mint a deterministic, signed `client_id` for the given metadata. Callers
    /// are expected to have already validated the redirect URIs.
    pub fn register(&self, payload: RegisterClientPayload) -> RegisteredClient {
        let mut redirect_uris: Vec<String> = payload
            .redirect_uris
            .iter()
            .map(|uri| normalize_redirect_uri(uri))
            .collect();
        redirect_uris.sort();
        redirect_uris.dedup();

        let meta = CanonicalMeta {
            client_name: payload.client_name,
            redirect_uris,
        };
        let client_id = self.encode(&meta);
        RegisteredClient {
            client_id,
            client_name: meta.client_name,
            redirect_uris: meta.redirect_uris,
        }
    }

    /// Resolve a `client_id` that we minted. Returns `None` when the id is not
    /// one of ours (no `ppcli.` prefix) so the caller can fall back to CIMD;
    /// `Some(Err)` when the id carries our prefix but fails verification.
    pub fn resolve_signed(
        &self,
        client_id: &str,
    ) -> Option<Result<ResolvedClient, ClientRegistrationError>> {
        if !client_id.starts_with("ppcli.") {
            return None;
        }
        Some(self.decode(client_id))
    }

    fn encode(&self, meta: &CanonicalMeta) -> String {
        let key = self
            .keys
            .get(&self.active_kid)
            .expect("active key present by construction");
        let meta_json = serde_json::to_vec(meta).expect("canonical meta serializes");
        let kid_b64 = URL_SAFE_NO_PAD.encode(self.active_kid.as_bytes());
        let meta_b64 = URL_SAFE_NO_PAD.encode(meta_json);
        let signing_input = format!("{PREFIX}.{kid_b64}.{meta_b64}");
        let tag = sign(key, &signing_input);
        format!("{signing_input}.{tag}")
    }

    fn decode(&self, client_id: &str) -> Result<ResolvedClient, ClientRegistrationError> {
        // ppcli . v1 . <kid> . <meta> . <tag>
        let parts: Vec<&str> = client_id.split('.').collect();
        if parts.len() != 5 || parts[0] != "ppcli" || parts[1] != "v1" {
            return Err(ClientRegistrationError::Malformed);
        }
        let kid_bytes = URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|_| ClientRegistrationError::Malformed)?;
        let kid = String::from_utf8(kid_bytes).map_err(|_| ClientRegistrationError::Malformed)?;
        let key = self
            .keys
            .get(&kid)
            .ok_or(ClientRegistrationError::UnknownKey)?;
        let tag = URL_SAFE_NO_PAD
            .decode(parts[4])
            .map_err(|_| ClientRegistrationError::Malformed)?;

        let signing_input = format!("{}.{}.{}.{}", parts[0], parts[1], parts[2], parts[3]);
        if !verify(key, &signing_input, &tag) {
            return Err(ClientRegistrationError::InvalidSignature);
        }

        let meta_json = URL_SAFE_NO_PAD
            .decode(parts[3])
            .map_err(|_| ClientRegistrationError::Malformed)?;
        let meta: CanonicalMeta =
            serde_json::from_slice(&meta_json).map_err(|_| ClientRegistrationError::Malformed)?;
        Ok(ResolvedClient {
            client_name: meta.client_name,
            redirect_uris: meta.redirect_uris,
        })
    }
}

fn derive_key(secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(KEY_DERIVATION_LABEL);
    hasher.update(secret.as_bytes());
    hasher.finalize().into()
}

fn sign(key: &[u8; 32], signing_input: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts a 32-byte key");
    mac.update(signing_input.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

fn verify(key: &[u8; 32], signing_input: &str, tag: &[u8]) -> bool {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts a 32-byte key");
    mac.update(signing_input.as_bytes());
    // `verify_slice` is constant-time.
    mac.verify_slice(tag).is_ok()
}

/// Strip the port from loopback-http redirect URIs so an ephemeral OS-assigned
/// port never enters the `client_id` (which would break dedup); leave https and
/// anything unparseable untouched. Consistent with the port-ignoring loopback
/// logic in `services::oauth::redirect_uri_matches`.
fn normalize_redirect_uri(uri: &str) -> String {
    let Ok(mut url) = Url::parse(uri) else {
        return uri.to_owned();
    };
    if is_loopback_http(&url) {
        let _ = url.set_port(None);
        return url.to_string();
    }
    uri.to_owned()
}

fn is_loopback_http(url: &Url) -> bool {
    url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registrar_with(active: &str, kids: &[&str]) -> ClientRegistrar {
        let mut keys = HashMap::new();
        for (index, kid) in kids.iter().enumerate() {
            // Distinct, deterministic per-kid key material.
            keys.insert((*kid).to_owned(), [index as u8 + 1; 32]);
        }
        ClientRegistrar {
            keys,
            active_kid: active.to_owned(),
        }
    }

    fn registrar() -> ClientRegistrar {
        registrar_with("k1", &["k1"])
    }

    fn payload(name: &str, uris: &[&str]) -> RegisterClientPayload {
        RegisterClientPayload {
            client_name: name.to_owned(),
            redirect_uris: uris.iter().map(|u| (*u).to_owned()).collect(),
        }
    }

    #[test]
    fn sign_verify_round_trip() {
        let reg = registrar();
        let registered = reg.register(payload("Codex CLI", &["http://localhost:1455/callback"]));
        assert!(registered.client_id.starts_with("ppcli.v1."));

        let resolved = reg
            .resolve_signed(&registered.client_id)
            .expect("id carries our prefix")
            .expect("id verifies");
        assert_eq!(resolved.client_name, "Codex CLI");
        assert_eq!(resolved.redirect_uris, registered.redirect_uris);
    }

    #[test]
    fn tampered_meta_is_rejected() {
        let reg = registrar();
        let id = reg
            .register(payload("Codex CLI", &["http://localhost:1455/callback"]))
            .client_id;
        let parts: Vec<&str> = id.split('.').collect();

        // Re-sign nothing: swap in a different meta but keep the original tag.
        let forged_meta = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&CanonicalMeta {
                client_name: "Evil".to_owned(),
                redirect_uris: vec!["http://localhost/callback".to_owned()],
            })
            .unwrap(),
        );
        let forged = format!("ppcli.v1.{}.{}.{}", parts[2], forged_meta, parts[4]);

        assert!(matches!(
            reg.resolve_signed(&forged),
            Some(Err(ClientRegistrationError::InvalidSignature))
        ));
    }

    #[test]
    fn foreign_client_id_is_not_ours() {
        let reg = registrar();
        assert!(reg
            .resolve_signed("https://client.example/metadata.json")
            .is_none());
    }

    #[test]
    fn loopback_port_is_normalized_away() {
        let reg = registrar();
        let registered = reg.register(payload("Codex CLI", &["http://localhost:1455/callback"]));
        assert_eq!(registered.redirect_uris, ["http://localhost/callback"]);
    }

    #[test]
    fn https_redirect_is_preserved() {
        let reg = registrar();
        let registered = reg.register(payload("Web Client", &["https://client.example/cb"]));
        assert_eq!(registered.redirect_uris, ["https://client.example/cb"]);
    }

    #[test]
    fn same_client_different_ephemeral_port_yields_identical_id() {
        let reg = registrar();
        let first = reg.register(payload("Codex CLI", &["http://localhost:1455/callback"]));
        let second = reg.register(payload("Codex CLI", &["http://localhost:1456/callback"]));
        assert_eq!(first.client_id, second.client_id);
    }

    #[test]
    fn unknown_signing_key_is_rejected() {
        let signer = registrar_with("ka", &["ka"]);
        let id = signer
            .register(payload("Codex CLI", &["http://localhost:1455/callback"]))
            .client_id;

        let other = registrar_with("kb", &["kb"]);
        assert!(matches!(
            other.resolve_signed(&id),
            Some(Err(ClientRegistrationError::UnknownKey))
        ));
    }

    #[test]
    fn structurally_malformed_id_is_rejected() {
        let reg = registrar();
        assert!(matches!(
            reg.resolve_signed("ppcli.v1.only-three"),
            Some(Err(ClientRegistrationError::Malformed))
        ));
    }
}
