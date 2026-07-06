use std::fmt;

use sha2::{Digest, Sha256};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    pub fn digest(value: &[u8]) -> Self {
        Self(Sha256::digest(value).into())
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Sha256Digest([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::Sha256Digest;

    #[test]
    fn digest_uses_sha256_and_redacts_debug_output() {
        let digest = Sha256Digest::digest(b"abc");

        assert_eq!(
            hex::encode(digest.as_bytes()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(format!("{digest:?}"), "Sha256Digest([redacted])");
    }

    #[test]
    fn digest_can_be_constructed_from_persisted_bytes() {
        let bytes = [42; 32];

        assert_eq!(Sha256Digest::from_bytes(bytes).as_bytes(), &bytes);
    }
}
