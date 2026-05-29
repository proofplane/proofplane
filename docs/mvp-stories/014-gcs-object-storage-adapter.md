# 014 - Object Storage Adapters

## Goal

Add object storage abstraction, a filesystem implementation for local/test use, and a GCS implementation for production evidence attachments.

## Design

Define a trait for object storage with static dispatch:

```rust
pub trait ObjectStore {
    async fn put_object(&self, request: PutObjectRequest) -> Result<ObjectMetadata, StorageError>;
    async fn get_object(&self, key: ObjectKey) -> Result<ObjectStream, StorageError>;
    async fn delete_object(&self, key: ObjectKey) -> Result<(), StorageError>;
}
```

Support production GCS and local filesystem configuration. Object keys should include workspace ID and stable prefixes. Metadata should include content type, content length, and checksum.

The filesystem adapter is the default for local development and automated tests. It should store object bytes under a configured root directory and keep metadata in either sidecar files or a deterministic local metadata representation. The GCS adapter is selected for live environments by configuration and should use native GCS APIs.

## Acceptance Criteria

- Storage abstraction supports upload, download, delete, metadata, and signed or internal retrieval strategy.
- Filesystem implementation reads a root directory from config and creates it if needed.
- Filesystem implementation is safe for isolated integration-test temp directories.
- GCS implementation reads settings from config.
- Uploads calculate and persist checksums.
- Evidence attachments can reference object metadata without storing file bytes in Postgres.
- Seed data can create sample object metadata and optionally sample objects.

## Tests

- Unit tests with fake object store cover service behavior.
- Integration tests upload, read, and delete objects through the filesystem implementation.
- GCS adapter tests use fakes or mocked boundaries until a staging GCS test is explicitly added.
- Tests verify checksum mismatch is detected.
- Tests verify object keys are workspace-scoped.

## QA Guide

1. Configure local object storage with a temp filesystem root.
2. Run storage integration tests.
3. Upload a sample evidence file.
4. Verify metadata and checksum.
5. Delete the object and confirm retrieval fails clearly.
