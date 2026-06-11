# 002 - GCS Object Store

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#object-store-runtime)

**Summary** - Implement GCS upload, read, metadata, copy, and delete behavior so
production attachments use the same logical object contract as local storage.

**Acceptance criteria**

- [ ] Given valid GCS configuration and credentials, when object operations run,
  then bytes and metadata round-trip under the configured prefix.
- [ ] Given missing credentials, permission denial, or unavailable storage,
  when an operation runs, then a classified storage error is returned without
  leaking credential material.
- [ ] Given an invalid or cross-workspace logical key, when any backend is used,
  then it is rejected before an SDK request.

**Tasks**

- [ ] Add GCS client construction and error mapping.
- [ ] Implement streaming put/get/head/copy/delete.
- [ ] Apply configured physical prefix without changing persisted logical keys.
- [ ] Run shared contract tests against filesystem and a GCS emulator.
- [ ] Add finalization integrity coverage for GCS copy.
