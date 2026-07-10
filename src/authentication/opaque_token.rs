use std::fmt;

use crc32fast::Hasher;
use secrecy::SecretString;
use sha2::{Digest, Sha256};

pub const PREFIX: &str = "ppat_";
pub const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
pub const RANDOM_LENGTH: usize = 30;
pub const CHECKSUM_LENGTH: usize = 6;
pub const TOKEN_LENGTH: usize = PREFIX.len() + RANDOM_LENGTH + CHECKSUM_LENGTH;

const ACCEPTED_RANDOM_BYTE_BOUND: u8 = 248;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AuditorInviteSecretDigest([u8; 32]);

impl AuditorInviteSecretDigest {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for AuditorInviteSecretDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditorInviteSecretDigest([redacted])")
    }
}

#[derive(Debug)]
pub struct GeneratedAuditorInviteSecret {
    pub raw_secret: SecretString,
    pub digest: AuditorInviteSecretDigest,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OpaqueTokenError {
    #[error("opaque secret generation failed")]
    Generation,
    #[error("malformed opaque secret")]
    Malformed,
    #[error("opaque secret checksum mismatch")]
    ChecksumMismatch,
}

pub fn generate_auditor_invite_secret() -> Result<GeneratedAuditorInviteSecret, OpaqueTokenError> {
    let secret = generate_secret_string()?;
    let digest = AuditorInviteSecretDigest(digest_secret(&secret));
    Ok(GeneratedAuditorInviteSecret {
        raw_secret: SecretString::from(secret),
        digest,
    })
}

pub fn parse_auditor_invite_secret(
    raw_secret: &str,
) -> Result<AuditorInviteSecretDigest, OpaqueTokenError> {
    parse_digest(raw_secret).map(AuditorInviteSecretDigest)
}

fn parse_digest(raw_token: &str) -> Result<[u8; 32], OpaqueTokenError> {
    if raw_token.len() != TOKEN_LENGTH || !raw_token.starts_with(PREFIX) {
        return Err(OpaqueTokenError::Malformed);
    }

    if !raw_token[PREFIX.len()..]
        .bytes()
        .all(|byte| ALPHABET.contains(&byte))
    {
        return Err(OpaqueTokenError::Malformed);
    }

    let checksum_start = PREFIX.len() + RANDOM_LENGTH;
    let expected = get_checksum(&raw_token[..checksum_start]);
    if raw_token[checksum_start..] != expected {
        return Err(OpaqueTokenError::ChecksumMismatch);
    }

    Ok(digest_secret(raw_token))
}

fn generate_secret_string() -> Result<String, OpaqueTokenError> {
    let random_body =
        generate_token_body(getrandom::fill).map_err(|_| OpaqueTokenError::Generation)?;

    let mut token = String::with_capacity(TOKEN_LENGTH);
    token.push_str(PREFIX);
    token.push_str(&random_body);
    token.push_str(&get_checksum(&token));

    Ok(token)
}

fn digest_secret(raw_token: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    hasher.finalize().into()
}

fn get_checksum(input: &str) -> String {
    let mut hasher = Hasher::new();
    hasher.update(input.as_bytes());
    encode_checksum_base62(hasher.finalize())
}

fn encode_checksum_base62(value: u32) -> String {
    let mut encoded = [ALPHABET[0]; CHECKSUM_LENGTH];
    let mut remaining = value;

    for index in (0..CHECKSUM_LENGTH).rev() {
        encoded[index] = ALPHABET[(remaining % ALPHABET.len() as u32) as usize];
        remaining /= ALPHABET.len() as u32;
    }

    encoded.iter().map(|byte| char::from(*byte)).collect()
}

fn generate_token_body<F, E>(mut fill: F) -> Result<String, E>
where
    F: FnMut(&mut [u8]) -> Result<(), E>,
{
    let mut random_body = String::with_capacity(RANDOM_LENGTH);
    let mut buffer = [0; RANDOM_LENGTH];

    while random_body.len() < RANDOM_LENGTH {
        fill(&mut buffer)?;
        for byte in buffer {
            if byte >= ACCEPTED_RANDOM_BYTE_BOUND {
                continue;
            }

            random_body.push(ALPHABET[(byte % ALPHABET.len() as u8) as usize] as char);
            if random_body.len() == RANDOM_LENGTH {
                break;
            }
        }
    }

    Ok(random_body)
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::{
        encode_checksum_base62, generate_auditor_invite_secret, generate_token_body,
        parse_auditor_invite_secret, OpaqueTokenError, ALPHABET, CHECKSUM_LENGTH, PREFIX,
        RANDOM_LENGTH, TOKEN_LENGTH,
    };

    #[test]
    fn generated_secret_has_required_shape_checksum_and_stable_digest() {
        let generated = generate_auditor_invite_secret().expect("opaque secret is generated");
        let raw_secret = generated.raw_secret.expose_secret();

        assert_eq!(raw_secret.len(), TOKEN_LENGTH);
        assert!(raw_secret.starts_with(PREFIX));
        assert!(raw_secret[PREFIX.len()..]
            .bytes()
            .all(|byte| ALPHABET.contains(&byte)));

        let parsed_digest =
            parse_auditor_invite_secret(raw_secret).expect("generated secret parses");
        assert_eq!(parsed_digest, generated.digest);
        assert_eq!(parsed_digest.as_bytes().len(), 32);
    }

    #[test]
    fn checksum_encoding_uses_six_zero_padded_base62_characters() {
        assert_eq!(encode_checksum_base62(0xcbf43926), "3jZRME");
        assert_eq!(encode_checksum_base62(0).len(), CHECKSUM_LENGTH);
        assert_eq!(encode_checksum_base62(0), "000000");
    }

    #[test]
    fn parser_rejects_malformed_tokens() {
        for raw_secret in [
            "",
            "xpat_000000000000000000000000000000000000",
            "ppat_00000000000000000000000000000000000",
            "ppat_0000000000000000000000000000000000000",
            "ppat_00000000000000000000000000000000000_",
            "v4.public.example",
        ] {
            assert_eq!(
                parse_auditor_invite_secret(raw_secret),
                Err(OpaqueTokenError::Malformed),
                "{raw_secret:?} should be malformed"
            );
        }
    }

    #[test]
    fn parser_rejects_altered_random_or_checksum_characters() {
        let generated = generate_auditor_invite_secret().expect("opaque secret is generated");
        let raw_secret = generated.raw_secret.expose_secret();

        let mut altered_random = raw_secret.to_owned();
        let replacement = different_base62_byte(altered_random.as_bytes()[PREFIX.len()]);
        altered_random.replace_range(
            PREFIX.len()..PREFIX.len() + 1,
            &(replacement as char).to_string(),
        );
        assert_eq!(
            parse_auditor_invite_secret(&altered_random),
            Err(OpaqueTokenError::ChecksumMismatch)
        );

        let mut altered_checksum = raw_secret.to_owned();
        let checksum_index = PREFIX.len() + RANDOM_LENGTH;
        let replacement = different_base62_byte(altered_checksum.as_bytes()[checksum_index]);
        altered_checksum.replace_range(
            checksum_index..checksum_index + 1,
            &(replacement as char).to_string(),
        );
        assert_eq!(
            parse_auditor_invite_secret(&altered_checksum),
            Err(OpaqueTokenError::ChecksumMismatch)
        );
    }

    #[test]
    fn random_body_rejection_sampling_skips_high_bytes() {
        let chunks = [
            [
                248, 249, 250, 251, 252, 253, 254, 255, 0, 61, 62, 123, 124, 185, 186, 247, 1, 2,
                3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14,
            ],
            [
                15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35,
                36, 37, 38, 39, 40, 41, 42, 43, 44,
            ],
        ];
        let mut chunk_index = 0;

        let body = generate_token_body(|buffer| {
            buffer.copy_from_slice(&chunks[chunk_index]);
            chunk_index += 1;
            Ok::<_, ()>(())
        })
        .expect("body is generated");

        assert_eq!(body, "0z0z0z0z123456789ABCDEFGHIJKLM");
        assert_eq!(chunk_index, 2);
    }

    #[test]
    fn debug_and_error_output_do_not_expose_secret_material() {
        let generated = generate_auditor_invite_secret().expect("opaque secret is generated");
        let raw_secret = generated.raw_secret.expose_secret();
        let digest_debug = format!("{:?}", generated.digest);
        let generated_debug = format!("{generated:?}");
        let error_debug = format!("{:?}", OpaqueTokenError::ChecksumMismatch);

        assert!(!digest_debug.contains(raw_secret));
        assert!(!generated_debug.contains(raw_secret));
        assert!(!error_debug.contains(raw_secret));
        assert_eq!(digest_debug, "AuditorInviteSecretDigest([redacted])");
    }

    fn different_base62_byte(byte: u8) -> u8 {
        if byte == ALPHABET[0] {
            ALPHABET[1]
        } else {
            ALPHABET[0]
        }
    }
}
