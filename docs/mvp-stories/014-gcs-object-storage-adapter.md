# 014 - GCS Object Storage Adapter

## Goal

Add object storage abstraction and GCS implementation for evidence attachments.

## Design

Define a trait for object storage with static dispatch:

```rust
pub trait ObjectStore {
    async fn put_object(&self, request: PutObjectRequest) -> Result<ObjectMetadata, StorageError>;
    async fn get_object(&self, key: ObjectKey) -> Result<ObjectStream, StorageError>;
    async fn delete_object(&self, key: ObjectKey) -> Result<(), StorageError>;
}
```

Support production GCS and local emulator configuration. Object keys should include workspace ID and stable prefixes. Metadata should include content type, content length, checksum, and provenance where applicable.

## Acceptance Criteria

- Storage abstraction supports upload, download, delete, metadata, and signed or internal retrieval strategy.
- GCS implementation reads settings from config.
- Uploads calculate and persist checksums.
- Evidence attachments can reference object metadata without storing file bytes in Postgres.
- Seed data can create sample object metadata and optionally sample objects.

## Tests

- Unit tests with fake object store cover service behavior.
- Integration tests upload, read, and delete objects through the local emulator.
- Tests verify checksum mismatch is detected.
- Tests verify object keys are workspace-scoped.

## QA Guide

1. Start local object storage emulator.
2. Run storage integration tests.
3. Upload a sample evidence file.
4. Verify metadata and checksum.
5. Delete the object and confirm retrieval fails clearly.
