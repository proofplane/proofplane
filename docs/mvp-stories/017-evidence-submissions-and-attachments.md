# 017 - Evidence Submissions and Attachments

## Goal

Allow actors to submit evidence against existing Evidence Requests, including attachment metadata and object storage references.

## Design

Evidence submissions support:

- Evidence Request ID
- attachment reference, URL, text, or structured payload
- submitter actor
- actor type
- system receipt timestamp
- evidence effective or coverage date
- source system
- collection method
- provenance metadata
- checksum or hash
- approval status
- replacement or supplement relationship

Attachments use GCS/object storage for bytes and Postgres for metadata. Submissions inherit Evidence Request-control mappings.

Document uploads must pass through a pluggable malware scanning boundary before
they can be treated as usable evidence. Model scanning as an adapter, not as a
ClamAV-specific domain concept:

```rust
pub trait MalwareScanner {
    async fn scan_bytes(&self, request: ScanBytesRequest) -> Result<MalwareScanResult, MalwareScanError>;
    async fn scan_object(&self, request: ScanObjectRequest) -> Result<MalwareScanResult, MalwareScanError>;
}
```

Attachment metadata should persist scan state independently from upload state:

- `pending`
- `clean`
- `malicious`
- `failed`
- `skipped`, only for explicitly allowed non-file evidence types

The MVP implementation should include:

- a `NoopMalwareScanner` for tests that do not exercise scanning behavior
- a ClamAV-backed scanner for local/self-hosted deployments
- configuration that can select scanner implementation and scanner timeouts
- enough adapter shape to support cloud-provider native scanning or commercial
  scanning APIs later without changing attachment domain records

Uploaded file attachments must not be approved, downloaded, or used as approved
source material until their malware scan status is `clean`. Malicious uploads
must remain auditable, but should not be served back to users through normal
download paths.

Submissions must distinguish system receipt time from the evidence effective or coverage date. Late submissions must not shift future Evidence Request due dates; cadence remains schedule-owned through the request's `schedule_anchor_at`.

## Acceptance Criteria

- Submission and attachment tables are migrated.
- API supports creating a submission, uploading or registering an attachment, getting submission status, and retrieving latest submission for a requirement.
- Service validates Evidence Request existence and workspace ownership.
- Submissions start in a pending approval status.
- Attachment metadata records malware scan status, scanner name/version where available, scan timestamp, and failure reason where safe to expose.
- File attachments enter a non-usable pending scan state until the configured malware scanner returns `clean`.
- Malicious or failed scans block approval, normal download, and approved-source-material use.
- The scanner boundary supports ClamAV for local/self-hosted operation and does not make ClamAV part of the domain model.
- Submission creation emits outbox and audit events.
- Seed data includes at least one pending submission and one attachment metadata record.

## Tests

- Domain tests cover status transitions and replacement/supplement rules.
- Domain/service tests cover attachment scan state transitions and blocking rules.
- Repository integration tests cover submission creation and latest-submission queries.
- Storage integration tests cover attachment object upload and retrieval.
- Scanner adapter tests cover clean, malicious, failed, timeout, and skipped outcomes.
- API integration tests cover valid submission, invalid request accumulation, missing Evidence Request, and latest submission.
- API integration tests verify unscanned, malicious, and failed-scan attachments cannot be approved or downloaded through normal paths.
- Tests verify submission inherits Evidence Request mappings indirectly rather than copying mapping rows.

## QA Guide

1. Start API with filesystem-backed object storage config.
2. Submit evidence against a seeded Evidence Request.
3. Start the configured malware scanner, such as local ClamAV.
4. Upload or register an attachment.
5. Confirm the attachment moves from `pending` to `clean`.
6. Retrieve submission status.
7. Query latest submission for the requirement.
8. Upload an EICAR test file in a local-only environment and confirm it is marked
   malicious and blocked from normal download.
