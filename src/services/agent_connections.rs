//! Shared machine authentication context retained while transport services are
//! decomposed into concrete application handlers.

pub use crate::authentication::AgentConnectionContext;

use crate::domain::Sha256Digest;

pub fn digest_secret(value: &str) -> Sha256Digest {
    Sha256Digest::digest(value.as_bytes())
}
