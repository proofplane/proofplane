use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    io,
    path::PathBuf,
};

use super::{ObjectKey, ObjectMetadata, ObjectStore, PutObjectRequest, StorageError};

#[derive(Debug, Clone)]
pub struct FilesystemObjectStore {
    root: PathBuf,
}

impl FilesystemObjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn object_path(&self, key: &ObjectKey) -> Result<PathBuf, StorageError> {
        Ok(self.root.join(key.relative_path()?))
    }
}

impl ObjectStore for FilesystemObjectStore {
    async fn put_object(&self, request: PutObjectRequest) -> Result<ObjectMetadata, StorageError> {
        let path = self.object_path(&request.key)?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&path, &request.bytes)?;

        Ok(ObjectMetadata {
            key: request.key,
            content_type: request.content_type,
            content_length: request.bytes.len() as u64,
            checksum: checksum(&request.bytes),
        })
    }

    async fn get_object(&self, key: &ObjectKey) -> Result<Vec<u8>, StorageError> {
        Ok(fs::read(self.object_path(key)?)?)
    }

    async fn delete_object(&self, key: &ObjectKey) -> Result<(), StorageError> {
        match fs::remove_file(self.object_path(key)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn checksum(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::{FilesystemObjectStore, ObjectKey, ObjectStore, PutObjectRequest, StorageError};
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[tokio::test]
    async fn filesystem_store_writes_reads_and_deletes_object() {
        let root = temp_storage_root();
        let store = FilesystemObjectStore::new(&root);
        let key = ObjectKey::new("workspace/evidence.txt");

        let metadata = store
            .put_object(PutObjectRequest {
                key: key.clone(),
                content_type: "text/plain".to_owned(),
                bytes: b"evidence".to_vec(),
            })
            .await
            .expect("object is stored");

        assert_eq!(metadata.key, key);
        assert_eq!(metadata.content_type, "text/plain");
        assert_eq!(metadata.content_length, 8);
        assert!(!metadata.checksum.is_empty());
        assert_eq!(
            store.get_object(&key).await.expect("object is read"),
            b"evidence"
        );

        store.delete_object(&key).await.expect("object is deleted");

        assert!(matches!(
            store.get_object(&key).await,
            Err(StorageError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn filesystem_store_rejects_path_traversal_keys() {
        let root = temp_storage_root();
        let store = FilesystemObjectStore::new(&root);

        let result = store
            .put_object(PutObjectRequest {
                key: ObjectKey::new("../outside.txt"),
                content_type: "text/plain".to_owned(),
                bytes: b"bad".to_vec(),
            })
            .await;

        assert!(matches!(result, Err(StorageError::InvalidKey(_))));

        let _ = fs::remove_dir_all(root);
    }

    fn temp_storage_root() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!("proofplane-storage-test-{nanos}"))
    }
}
