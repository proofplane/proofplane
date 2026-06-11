# Evidence Lifecycle Completion Spec

## Goal

Complete the REST evidence lifecycle on top of the implemented submission,
quarantine, ClamAV scan, and finalization pipeline. Callers must be able to find
the latest submission, retrieve finalized attachment bytes, and exercise the
flow from seeded local data.

## Current Implementation

- Submission create and detail APIs are implemented.
- Multipart uploads require CRC32C and write to filesystem quarantine storage.
- Attachment creation and `attachment.scan_requested` enqueue atomically.
- ClamAV scanning and finalization are idempotent worker deliveries.
- Repository code already implements and integration-tests
  `latest_evidence_submission_for_request`.
- There is no latest-submission route, attachment download route, or seeded
  submission/object.

## API Contract

Add:

```text
GET /workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions/latest
GET /workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/content
```

The latest endpoint returns the existing submission-detail shape. It orders by
`received_at DESC, id DESC` and returns `404` when the Evidence Request is
missing, belongs to another workspace, or has no submissions.

The content endpoint streams bytes with the stored content type and a safe
`Content-Disposition` filename. It returns `404` for cross-workspace,
cross-submission, or missing attachments. It returns `409 Conflict` with a
stable `attachment_not_ready` problem for `pending` and `finalizing`.
`contains_virus` and `failed` return `404` so normal callers cannot use the
endpoint to confirm or recover quarantined content.

## Retrieval Invariants

The repository loads a download candidate only when all path identifiers join
through one workspace. The service then requires:

- `upload_status = uploaded`;
- an object key under the stable non-quarantine submission prefix;
- object metadata matching stored content type, length, and SHA-256.

Metadata mismatch is an internal storage-integrity failure, not downloadable
content. Attachment bytes are streamed; the API does not buffer the full object.

This is the reusable eligibility rule for later source-material and packet work.
Those features may reference only uploaded attachments with finalized keys.

## Demo Seed

The seed command creates one deterministic submission for a seeded Evidence
Request, one uploaded attachment row, and matching filesystem object metadata
and bytes when the configured backend is filesystem. Re-running seed updates or
reuses the deterministic records without duplicate submissions or attachments.

GCS seeding is not required; production data is created through normal APIs.

## Audit Logging

Submission creation and attachment acceptance emit structured
`type = "audit_log"` records after their database transactions commit. The logs
include actor, workspace, request, operation, and affected object identifiers,
but never attachment bytes, API keys, or raw object metadata sidecars.

## Revisions

- 2026-06-11: Extracted remaining story 017 work after verifying scan and
  finalization are implemented.
- 2026-06-11: Replaced database-backed audit events with structured application
  audit logs.
