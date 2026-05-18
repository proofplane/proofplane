use std::{
    future::Future,
    io,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

mod local;

pub use local::FilesystemObjectStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectKey(String);

impl ObjectKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn relative_path(&self) -> Result<PathBuf, StorageError> {
        let path = Path::new(&self.0);

        if path.is_absolute() {
            return Err(StorageError::InvalidKey(self.0.clone()));
        }

        let mut relative = PathBuf::new();

        for component in path.components() {
            match component {
                Component::Normal(segment) => relative.push(segment),
                _ => return Err(StorageError::InvalidKey(self.0.clone())),
            }
        }

        if relative.as_os_str().is_empty() {
            return Err(StorageError::InvalidKey(self.0.clone()));
        }

        Ok(relative)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutObjectRequest {
    pub key: ObjectKey,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMetadata {
    pub key: ObjectKey,
    pub content_type: String,
    pub content_length: u64,
    pub checksum: String,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("invalid object key: {0}")]
    InvalidKey(String),
    #[error("object storage I/O error: {0}")]
    Io(#[from] io::Error),
}

pub trait ObjectStore {
    fn put_object(
        &self,
        request: PutObjectRequest,
    ) -> impl Future<Output = Result<ObjectMetadata, StorageError>> + Send;

    fn get_object(
        &self,
        key: &ObjectKey,
    ) -> impl Future<Output = Result<Vec<u8>, StorageError>> + Send;

    fn delete_object(
        &self,
        key: &ObjectKey,
    ) -> impl Future<Output = Result<(), StorageError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::{ObjectKey, StorageError};
    use std::io;

    #[test]
    fn stores_object_key() {
        let key = ObjectKey::new("workspace/evidence.txt");

        assert_eq!(key.as_str(), "workspace/evidence.txt");
    }

    #[test]
    fn invalid_key_error_display_is_stable() {
        let error = StorageError::InvalidKey("../outside.txt".to_owned());

        assert_eq!(error.to_string(), "invalid object key: ../outside.txt");
    }

    #[test]
    fn io_error_display_is_stable() {
        let error = StorageError::Io(io::Error::new(io::ErrorKind::NotFound, "missing object"));

        assert_eq!(
            error.to_string(),
            "object storage I/O error: missing object"
        );
    }
}
