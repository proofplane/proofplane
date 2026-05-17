use proofplane::storage::{FilesystemObjectStore, ObjectKey, ObjectStore, PutObjectRequest};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[tokio::test]
async fn filesystem_object_store_writes_reads_and_deletes_object() {
    let root = temp_storage_root();
    let store = FilesystemObjectStore::new(&root);
    let key = ObjectKey::new("workspace/integration.txt");

    let metadata = store
        .put_object(PutObjectRequest {
            key: key.clone(),
            content_type: "text/plain".to_owned(),
            bytes: b"integration evidence".to_vec(),
        })
        .await
        .expect("object is stored");

    assert_eq!(metadata.content_length, 20);
    assert_eq!(
        store.get_object(&key).await.expect("object is read"),
        b"integration evidence"
    );

    store.delete_object(&key).await.expect("object is deleted");

    let _ = fs::remove_dir_all(root);
}

fn temp_storage_root() -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is after unix epoch")
        .as_nanos();

    std::env::temp_dir().join(format!("proofplane-integration-storage-{nanos}"))
}
