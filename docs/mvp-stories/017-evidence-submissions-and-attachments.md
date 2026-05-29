# 017 - Evidence Submissions and Attachments

## Goal

Allow actors to submit evidence against existing Evidence Requests, including attachment metadata and object storage references.

## Design

Evidence submissions support:

- Evidence Request ID
- file uploads
- submitter actor
- system receipt timestamp
- evidence effective or coverage date
- source system
- collection method
- provenance metadata for source/integration receipt details
- submission-level and attachment-level checksums or hashes
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

Provenance is submission-level metadata that explains where the evidence came
from and how Proofplane received it. Examples include an integration run ID, a
source report URL, the upstream export timestamp, webhook delivery IDs, source
account IDs, or the query/filter parameters used to produce the evidence. It is
not the attachment byte-integrity record and should not duplicate object storage
metadata.

Attachment metadata should persist scan state independently from upload state.
Store scan state in a separate scan record so the attachment object metadata can
remain stable while scanner attempts, scanner versions, and failure details
change over time:

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

Uploaded file attachments must not be downloaded or used as source material
until their malware scan status is `clean`. Malicious uploads must remain
auditable, but should not be served back to users through normal download paths.

Evidence submissions do not have an approval lifecycle in the MVP. A submission
exists once the API accepts it; callers that need human or agent review should
perform that review before upload. Submission usability is derived from
attachment scan state rather than a persisted submission status.

Submissions must distinguish system receipt time from the evidence effective or coverage date. Late submissions must not shift future Evidence Request due dates; cadence remains schedule-owned through the request's `schedule_anchor_at`.

## Acceptance Criteria

- Submission and attachment tables are migrated.
- API supports creating a submission, uploading or registering an attachment, getting submission details, and retrieving latest submission for a requirement.
- Service validates Evidence Request existence and workspace ownership.
- File attachment uploads require caller-provided CRC32C and reject the upload when the received bytes do not match it.
- Attachment metadata records object references and byte-integrity checksums.
- Attachment scan records track malware scan status, scanner name/version where
  available, scan timestamp, and failure reason where safe to expose.
- File attachments enter a non-usable pending scan state until the configured malware scanner returns `clean`.
- Malicious or failed scans block normal download and source-material use.
- The scanner boundary supports ClamAV for local/self-hosted operation and does not make ClamAV part of the domain model.
- Submission creation emits outbox and audit events.
- Seed data includes at least one sample submission and one attachment metadata record.

## Tests

- Domain tests cover replacement/supplement rules and scan-state usability rules.
- Domain/service tests cover attachment scan state transitions and blocking rules.
- Repository integration tests cover submission creation and latest-submission queries.
- Storage integration tests cover attachment object upload and retrieval.
- Scanner adapter tests cover clean, malicious, failed, timeout, and skipped outcomes.
- API integration tests cover valid submission, invalid request accumulation, missing Evidence Request, and latest submission.
- API integration tests verify unscanned, malicious, and failed-scan attachments cannot be downloaded through normal paths.
- Tests verify submission inherits Evidence Request mappings indirectly rather than copying mapping rows.

## QA Guide

1. Start API with filesystem-backed object storage config.
2. Submit evidence against a seeded Evidence Request.
3. Start the configured malware scanner, such as local ClamAV.
4. Upload or register an attachment.
5. Confirm the attachment moves from `pending` to `clean`.
6. Retrieve submission details.
7. Query latest submission for the requirement.
8. Upload an EICAR test file in a local-only environment and confirm it is marked
   malicious and blocked from normal download.

## Implementation Slices

1. Submission domain and database model: define submission, attachment, and attachment scan IDs, replacement or supplement relationships, receipt time versus effective or coverage date, source system, collection method, provenance JSON, checksums, and separate attachment scan records.
2. Repository layer: support creating submissions, attaching metadata, reading submission details, querying latest submissions, and verifying workspace ownership through the linked Evidence Request.
3. Submission API without file uploads: add the basic REST surface for creating an accepted submission record and reading its details before introducing binary upload handling.
4. Attachment registration API: allow callers to register attachment metadata or existing object references separately from raw upload, including initial scan states such as `pending` or `skipped`.
5. File upload API and CRC32C validation: accept uploaded bytes, require caller-provided CRC32C, reject mismatches, write to object storage, and persist object metadata references.
6. Malware scanner boundary and noop implementation: add scanner request, result, and error types plus `NoopMalwareScanner` for tests and flows that do not exercise scanning behavior.
7. Scan state enforcement: block normal download and source-material use unless file attachments have scan status `clean`.
8. ClamAV adapter and scanner configuration: add the local/self-hosted scanner implementation, timeout settings, scanner selection, and adapter tests for clean, malicious, failed, timeout, and skipped outcomes.
9. Download and retrieval API: expose normal attachment download paths that refuse pending, malicious, and failed attachments.
10. Latest submission query/API, seed data, outbox, and audit polish: complete the latest-submission read path and add submission-created events, audit records, and seed sample submission plus attachment metadata.

Recommended implementation order:

1. Submission domain and database model.
2. Repository layer.
3. Submission API without file uploads.
4. Attachment registration API.
5. File upload API and CRC32C validation.
6. Malware scanner boundary and noop implementation.
7. Scan state enforcement.
8. ClamAV adapter and scanner configuration.
9. Download and retrieval API.
10. Latest submission query/API, seed data, outbox, and audit polish.
