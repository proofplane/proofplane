use std::fmt;

use http::HeaderValue;

use crate::validation::Validation;

use super::{validate_document_filename, DomainError, Sha256Digest};

const MAX_CONTENT_TYPE_BYTES: usize = 255;

#[derive(Clone, PartialEq, Eq)]
pub struct DeclaredUploadFile {
    filename: String,
    content_type: String,
    expected_content_length: u64,
    expected_sha256: Option<Sha256Digest>,
}

impl fmt::Debug for DeclaredUploadFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeclaredUploadFile([redacted])")
    }
}

impl DeclaredUploadFile {
    pub fn new(
        filename: String,
        content_type: String,
        expected_content_length: u64,
        expected_sha256: Option<String>,
        max_bytes: u64,
    ) -> Validation<Self, DomainError> {
        let mut errors = Vec::new();
        let filename = match validate_document_filename(filename).into_result() {
            Ok(filename) => Some(filename),
            Err(mut filename_errors) => {
                errors.append(&mut filename_errors);
                None
            }
        };
        let valid_content_type = !content_type.is_empty()
            && content_type.len() <= MAX_CONTENT_TYPE_BYTES
            && content_type.trim() == content_type
            && content_type.parse::<mime::Mime>().is_ok()
            && HeaderValue::from_str(&content_type).is_ok();
        if !valid_content_type {
            errors.push(DomainError::InvalidDocumentContentType);
        }
        let maximum = max_bytes.min(i64::MAX as u64);
        if expected_content_length > maximum {
            errors.push(DomainError::DocumentContentLengthTooLarge { maximum });
        }
        let expected_sha256 = match expected_sha256 {
            None => Some(None),
            Some(value) if is_lowercase_sha256(&value) => hex::decode(value)
                .ok()
                .and_then(|bytes| bytes.try_into().ok())
                .map(Sha256Digest::from_bytes)
                .map(Some),
            Some(_) => None,
        };
        if expected_sha256.is_none() {
            errors.push(DomainError::InvalidDocumentSha256Checksum);
        }

        match (filename, valid_content_type, expected_sha256) {
            (Some(filename), true, Some(expected_sha256)) if errors.is_empty() => {
                Validation::valid(Self {
                    filename,
                    content_type,
                    expected_content_length,
                    expected_sha256,
                })
            }
            _ => Validation::invalid_many(errors),
        }
    }

    pub(crate) fn rehydrate(
        filename: String,
        content_type: String,
        expected_content_length: u64,
        expected_sha256: Option<Sha256Digest>,
    ) -> Result<Self, DeclaredUploadFileError> {
        Self::new(
            filename,
            content_type,
            expected_content_length,
            expected_sha256.map(|digest| hex::encode(digest.as_bytes())),
            i64::MAX as u64,
        )
        .into_result()
        .map_err(|_| DeclaredUploadFileError::InvalidRehydration)
    }

    pub fn validate_declared(
        &self,
        content_type: &str,
        content_length: u64,
    ) -> Result<(), DeclaredUploadFileError> {
        if content_type != self.content_type {
            return Err(DeclaredUploadFileError::ContentTypeMismatch);
        }
        if content_length != self.expected_content_length {
            return Err(DeclaredUploadFileError::DeclaredContentLengthMismatch);
        }
        Ok(())
    }

    pub fn validate_staged(
        &self,
        content_length: i64,
        checksum_sha256: &str,
    ) -> Result<(), DeclaredUploadFileError> {
        let content_length = u64::try_from(content_length)
            .map_err(|_| DeclaredUploadFileError::ReceivedContentLengthMismatch)?;
        if content_length != self.expected_content_length {
            return Err(DeclaredUploadFileError::ReceivedContentLengthMismatch);
        }
        if self
            .expected_sha256
            .is_some_and(|expected| hex::encode(expected.as_bytes()) != checksum_sha256)
        {
            return Err(DeclaredUploadFileError::ChecksumMismatch);
        }
        Ok(())
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn expected_content_length(&self) -> u64 {
        self.expected_content_length
    }

    pub fn expected_sha256(&self) -> Option<&Sha256Digest> {
        self.expected_sha256.as_ref()
    }
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeclaredUploadFileError {
    #[error("persisted declared upload file is invalid")]
    InvalidRehydration,
    #[error("declared content type does not match")]
    ContentTypeMismatch,
    #[error("declared content length does not match")]
    DeclaredContentLengthMismatch,
    #[error("received content length does not match")]
    ReceivedContentLengthMismatch,
    #[error("received checksum does not match")]
    ChecksumMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_accepts_255_bytes() {
        let content_type = format!("application/{}", "a".repeat(243));
        assert_eq!(content_type.len(), 255);

        assert!(
            DeclaredUploadFile::new("file.bin".to_owned(), content_type, 1, None, 1)
                .into_result()
                .is_ok()
        );
    }

    #[test]
    fn content_type_rejects_256_bytes() {
        let content_type = format!("application/{}", "a".repeat(244));
        assert_eq!(content_type.len(), 256);

        assert_eq!(
            DeclaredUploadFile::new("file.bin".to_owned(), content_type, 1, None, 1).into_result(),
            Err(vec![DomainError::InvalidDocumentContentType])
        );
    }
}
