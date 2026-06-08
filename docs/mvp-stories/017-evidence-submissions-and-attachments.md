# 017 - Evidence Submissions and Attachments

## Status

Partially complete. Submission creation, submission detail retrieval, multipart
attachment upload, CRC32C validation, filesystem object writes, attachment
metadata with pending upload status, scanner dispatch through the transactional
outbox and Pub/Sub push worker path, and the object-based malware scanner
boundary with noop implementation are implemented. The worker now handles
`attachment.scan_requested` messages by scanning quarantined objects and
persisting terminal attachment upload status for malicious or failed outcomes.
Clean scans atomically transition attachments to `finalizing` and enqueue
`attachment.finalization_requested`; that handler copies the object, updates the
attachment to the final object key and `uploaded` status, and best-effort
deletes the quarantine copy. Scan and finalization handlers use the concrete
Postgres repository. Their database transitions, retry behavior, idempotency,
and failure rollback are covered by integration tests rather than repository
fakes in handler unit tests.
Download enforcement, malware scanner adapters, latest-submission API, audit
polish, and seed data remain open.

## Goal

Allow actors to submit evidence against existing Evidence Requests and upload file attachments through the API.

## Design

Evidence submissions support:

- Evidence Request ID
- file uploads
- submitter actor
- system receipt timestamp
- evidence effective or coverage date
- source system
- collection method
- submission-level and attachment-level checksums or hashes
- replacement or supplement relationship

Attachments represent files uploaded into Proofplane-managed object storage. API
callers do not register arbitrary existing object references in the MVP. The
multipart upload endpoint receives bytes, validates caller-provided integrity
metadata, writes the object to quarantine storage, records attachment upload
state in Postgres, and queues malware scanning through the transactional outbox.
Submissions inherit Evidence Request-control mappings.

The implemented scan dispatch slice emits an `attachment.scan_requested` event
when a quarantined upload is accepted. The event is appended in the same
database transaction as the pending attachment row, then
published by the standalone dequeuer to Pub/Sub and delivered to the worker's
push endpoint. The worker loads pending scan work and scans the quarantined
object. Clean results enqueue finalization in the same transaction as the
`pending` to `finalizing` state change. A separate worker delivery copies the
object to its final path and marks it `uploaded`. Duplicate or stale messages
at either stage are acknowledged no-ops.

Quarantined files are not usable evidence. When scanning returns `clean`, the
worker requests a separate finalization delivery. That delivery copies the file
to the final workspace-scoped object path in the same bucket and marks the
attachment usable. Workspaces should be organized by dedicated object key
prefixes rather than separate buckets in the MVP.

Document uploads must pass through a pluggable malware scanning boundary before
they can be treated as usable evidence. Model scanning as an adapter, not as a
ClamAV-specific domain concept:

```rust
pub trait MalwareScanner {
    async fn scan_object(&self, request: ScanObjectRequest) -> Result<MalwareScanResult, MalwareScanError>;
}
```

The current MVP stores the attachment lifecycle directly in
`evidence_attachments.upload_status`:

- `pending`
- `finalizing`
- `uploaded`
- `contains_virus`
- `failed`

The object key remains the quarantine key for `pending`, `contains_virus`, and
`failed` attachments. Finalization changes both the object key and status to the
stable path and `uploaded`. Scanner reasons are operational diagnostics: they
are emitted in structured handler logs with the attachment ID and quarantine
key, but are not persisted in Postgres or exposed in the API. Scanner
name/version and attempt history are likewise not durable MVP data.

The remaining scanner work should include:

- the existing `NoopMalwareScanner` for local/runtime flows that do not use a
  real scanner
- a ClamAV-backed scanner for local/self-hosted deployments
- configuration that can select scanner implementation and scanner timeouts
- enough adapter shape to support cloud-provider native scanning or commercial
  scanning APIs later without changing the attachment lifecycle contract

Uploaded file attachments must not be downloaded or used as source material
until their upload status is `uploaded`. Malicious uploads must remain
auditable, but should not be served back to users through normal download paths.

Evidence submissions do not have an approval lifecycle in the MVP. A submission
exists once the API accepts it; callers that need human or agent review should
perform that review before upload. Submission usability is derived from
attachment upload status rather than a persisted submission status.

Submissions must distinguish system receipt time from the evidence effective or coverage date. Late submissions must not shift future Evidence Request due dates; cadence remains schedule-owned through the request's `schedule_anchor_at`.

## Acceptance Criteria

- Submission and attachment tables are migrated.
- API supports creating a submission, uploading a file attachment with
  multipart/form-data, getting submission details, and retrieving latest
  submission for a requirement.
- Service validates Evidence Request existence and workspace ownership.
- File attachment uploads require caller-provided CRC32C and reject the upload when the received bytes do not match it.
- Attachment records are created only for files uploaded into Proofplane-managed
  storage, initially under a quarantine object key.
- Clean attachments are finalized under workspace-scoped object key prefixes.
- Attachment metadata records object references and byte-integrity checksums
  verified from the uploaded bytes.
- Upload acceptance queues scanner work through the transactional outbox and
  Pub/Sub delivery path.
- Attachment upload status tracks `pending`, `finalizing`, `uploaded`,
  `contains_virus`, and `failed` lifecycle states.
- Scanner failure and malicious-detection reasons are logged as structured
  operational diagnostics and are not persisted in the attachment model.
- File attachments enter a non-usable pending state until the configured
  malware scanner returns clean and finalization marks them `uploaded`.
- Malicious or failed scans block normal download and source-material use.
- The scanner boundary supports ClamAV for local/self-hosted operation and does not make ClamAV part of the domain model.
- Accepted uploads emit scan-request outbox events.
- Submission creation and upload acceptance audit events remain deferred to the
  audit story.
- Seed data includes at least one sample submission and one uploaded attachment
  metadata record.

## Tests

- Domain tests cover attachment upload-status parsing and invariants.
- Repository integration tests cover submission creation, submission details,
  latest-submission queries, attachment creation, pending work lookup, terminal
  status updates, and atomic/idempotent finalization handoff.
- Worker-handler integration tests use concrete Postgres to cover clean,
  malicious, failed, retryable, final-delivery, duplicate-delivery, and database
  rollback behavior. Scanner and object-store fakes remain at those external
  adapter boundaries; there are no generic attachment repository fakes.
- Storage integration tests cover quarantined attachment upload, clean-file
  finalization, and retrieval from the finalized object path.
- Scanner adapter tests cover clean, malicious, failed, and timeout outcomes.
- API integration tests cover valid submission, invalid request accumulation,
  missing Evidence Request, attachment upload, CRC32C validation, object writes,
  pending attachment status, and outbox scan-request payloads.
- API integration tests verify unscanned, malicious, and failed-scan attachments cannot be downloaded through normal paths.
- Tests verify submission inherits Evidence Request mappings indirectly rather than copying mapping rows.

## QA Guide

1. Start API with filesystem-backed object storage config.
2. Submit evidence against a seeded Evidence Request.
3. Start the configured malware scanner, such as local ClamAV.
4. Upload an attachment with multipart/form-data.
5. Confirm the attachment moves from `pending` through `finalizing` to
   `uploaded`, with its object key changed from quarantine to the stable path.
6. Retrieve submission details.
7. Query latest submission for the requirement.
8. Upload an EICAR test file in a local-only environment and confirm it is marked
   malicious and blocked from normal download.

## Implementation Slices

1. Submission domain and database model: define submission and attachment IDs,
   replacement or supplement relationships, receipt time versus effective or
   coverage date, source system, collection method, checksums, and attachment
   upload status.
2. Repository layer: support creating submissions, attaching metadata, reading submission details, querying latest submissions, and verifying workspace ownership through the linked Evidence Request.
3. Submission API without file uploads: add the basic REST surface for creating an accepted submission record and reading its details before introducing binary upload handling.
4. Multipart attachment upload API and CRC32C validation: accept uploaded bytes
   for an existing submission, require caller-provided CRC32C, reject
   mismatches, write the file to quarantine object storage, create attachment
   metadata with `pending` status, and return `202 Accepted`.
5. Scan dispatch through transactional outbox: enqueue attachment-scan work when a quarantined upload is accepted, publish it through the existing outbox to Pub/Sub flow, and keep scanner message payloads based on attachment IDs and quarantine object keys. Implemented.
6. Malware scanner boundary and noop implementation: add scanner request, result, and error types plus `NoopMalwareScanner` for tests and flows that do not exercise scanning behavior. Implemented.
7. Scan worker finalization: consume scan messages, scan quarantined objects,
   persist terminal attachment status, and use a separate finalization message
   to copy clean files to final workspace-scoped object paths; leave malicious
   or failed files quarantined and unusable. Implemented.
8. Scan state enforcement: block normal download and source-material use unless
   file attachments have `uploaded` status and a finalized object reference.
   Next active slice.
9. ClamAV adapter and scanner configuration: add the local/self-hosted scanner implementation, timeout settings, scanner selection, and adapter tests for clean, malicious, failed, and timeout outcomes.
10. Download and retrieval API: expose normal attachment download paths that  refuse pending, malicious, failed, and unfinalized attachments.
11. Latest submission query/API, seed data, and audit polish: complete the
latest-submission API surface, add audit records, and seed sample submission
plus uploaded attachment metadata.

Recommended implementation order:

1. Submission domain and database model.
2. Repository layer.
3. Submission API without file uploads.
4. Multipart attachment upload API and CRC32C validation.
5. Scan dispatch through transactional outbox. Implemented.
6. Malware scanner boundary and noop implementation. Implemented.
7. Scan worker finalization. Implemented.
8. Scan state enforcement. Next active slice.
9. ClamAV adapter and scanner configuration.
10. Download and retrieval API.
11. Latest submission query/API, seed data, and audit polish.
