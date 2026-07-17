# 002 - GCS Object Store

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#object-store-runtime)

**Summary** - Implement GCS upload, read, metadata, copy, and delete behavior so
production submissions use the same logical object contract as local storage.

**Acceptance criteria**

- [ ] Given valid GCS configuration and credentials, when object operations run,
  then bytes and metadata round-trip under the configured prefix.
- [ ] Given missing credentials, permission denial, or unavailable storage,
  when an operation runs, then a classified storage error is returned without
  leaking credential material.
- [ ] Given an invalid or cross-workspace logical key, when any backend is used,
  then it is rejected before an SDK request.
- [ ] Given the CI GCS test bucket, when the shared object-store contract runs,
  then upload, head, read, copy, delete, prefix isolation, and checksums pass
  against the real GCS implementation.
- [ ] Given local development without Google credentials, when integration tests
  run, then they use filesystem storage and do not attempt GCS access.

**Tasks**

- [ ] Add GCS client construction and error mapping.
- [ ] Implement streaming put/get/head/copy/delete.
- [ ] Apply configured physical prefix without changing persisted logical keys.
- [ ] Remove GCS emulator-only endpoint and anonymous credential configuration.
- [ ] Run the shared contract locally against filesystem and in CI against a
  real, isolated GCS test-bucket prefix.
- [ ] Make CI cleanup reliable and restrict the test identity to the test bucket.
- [ ] Add finalization integrity coverage for GCS copy.
