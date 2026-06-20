# 005 - Auditor Packet Export Jobs

**Status:** Todo · **Depends on:** 004, production-runtime-adapters/001, reliability-observability/005 · **Spec:** [spec.md](../spec.md#auditor-packet-read-model)

**Summary** - Create durable export jobs and have the background worker assemble
auditor-ready ZIPs into object storage without routing bytes through the
requesting agent or API process.

**Acceptance criteria**

- [ ] Given a valid packet selection, when export is requested, then a pending
  export and its outbox message commit atomically and the API returns `202`.
- [ ] Given a pending export, when the worker completes it, then a deterministic
  ZIP containing the manifest, Markdown, bounded summaries, and eligible
  attachments is stored and the export becomes `ready` with verified metadata.
- [ ] Given pending, malicious, failed, missing, or integrity-mismatched objects,
  when the worker runs, then the manifest records an allowed gap or the export
  fails before becoming downloadable.
- [ ] Given duplicate delivery or a retryable storage failure, when processing
  resumes, then one deterministic export object is produced without duplicate
  rows or conflicting archives.
- [ ] Given an export status read, when it succeeds, then it returns compact
  lifecycle metadata and bounded polling guidance without object keys, ZIP
  bytes, or attachment contents.

**Tasks**

- [ ] Add export records, lifecycle states, expiry, and transactional outbox
  creation.
- [ ] Add request/status services, authorization, REST routes, and audit events.
- [ ] Add worker dispatch and idempotent bounded-resource ZIP assembly.
- [ ] Store under a dedicated export prefix and persist verified object metadata.
- [ ] Add request, worker retry/idempotency, archive-content, and isolation tests.

**Notes**

- Production use also requires Production Runtime Adapters tickets 002 and 003
  for GCS and non-emulator Pub/Sub startup.
