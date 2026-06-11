# Evidence Lifecycle Completion Spec

## Goal

Complete the evidence lifecycle on top of the implemented submission,
quarantine, ClamAV scan, and finalization pipeline. Callers must be able to find
the latest submission, create a safe human download grant for finalized
attachment bytes, and exercise the flow from seeded local data.

## Current Implementation

- Submission create and detail APIs are implemented.
- Multipart uploads require CRC32C and write to filesystem quarantine storage.
- Attachment creation and `attachment.scan_requested` enqueue atomically.
- ClamAV scanning and finalization are idempotent worker deliveries.
- Repository code already implements and integration-tests
  `latest_evidence_submission_for_request`.
- There is no latest-submission route, human download-grant flow, or seeded
  submission/object.

## API Contract

Add:

```text
GET /workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions/latest
POST /workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/download-grants
GET /attachment-downloads/{token}
```

The latest endpoint returns the existing submission-detail shape. It orders by
`received_at DESC, id DESC` and returns `404` when the Evidence Request is
missing, belongs to another workspace, or has no submissions.

The grant endpoint authenticates and authorizes the workspace actor, verifies
attachment eligibility, and returns an opaque HTTPS URL intended for human
inspection. The grant is scoped to one attachment and expires after five
minutes. It may be downloaded more than once before expiry so browser retries,
link previews, and interrupted transfers do not permanently invalidate it. The
response includes expiry, filename, content type, and content length, but not the
object key or storage backend.

The download route is outside workspace-authenticated routes. Possession of the
opaque token is the authorization, so the URL is a short-lived bearer secret. A
valid GET streams bytes immediately with the stored content type, a safe
`Content-Disposition` filename, `Cache-Control: private, no-store`, and
`Referrer-Policy: no-referrer`.

Expired, unknown, malicious, failed, missing, cross-submission, and
cross-workspace grants return `404`. Pending and finalizing attachments return
`409 attachment_not_ready` at grant creation; no grant is issued. Every GET
rechecks the current attachment and object metadata, so an attachment that later
becomes ineligible stops downloading even before the grant expires.

## Download Grant Model

Persist a grant row containing:

- grant ID;
- token hash, never the raw token;
- workspace, submission, attachment, and issuing actor IDs;
- expiry and creation timestamps.

Generate at least 256 bits of cryptographically secure token entropy. The
download URL contains the raw token only in its path. Do not put it in query
parameters, structured application logs, analytics, or referrer-bearing pages.
Raw request URLs may still be visible to infrastructure access logging, browser
history, link-preview systems, and endpoint security software. The five-minute
expiry limits the value of exposure; production ingress should redact or
exclude the download path where supported and restrict access to unavoidable
request logs.

Grant creation proves only that an authorized workspace actor requested human
access. Download GETs do not identify the human opening the URL. Human-level
attribution would require an interactive login and is explicitly not part of
this minimal browserless flow.

## Retrieval Invariants

The repository loads a grant candidate only when all path identifiers join
through one workspace. Grant creation requires:

- `upload_status = uploaded`;
- an object key under the stable non-quarantine submission prefix;
- object metadata matching stored content type, length, and SHA-256.

Each download reloads the attachment and verifies the status, final key, and
object metadata again. Metadata mismatch is an internal storage-integrity
failure, not downloadable content. Attachment bytes are streamed through
Proofplane; the API does not buffer the full object and does not expose a GCS
signed URL.

This is the reusable eligibility rule for later source-material and packet work.
Those features may reference only uploaded attachments with finalized keys and
must create their own grants when offering human download links.

## Demo Seed

The seed command creates one deterministic submission for a seeded Evidence
Request, one uploaded attachment row, and matching filesystem object metadata
and bytes when the configured backend is filesystem. Re-running seed updates or
reuses the deterministic records without duplicate submissions or attachments.

GCS seeding is not required; production data is created through normal APIs.

## Audit Logging

Submission creation, attachment acceptance, grant creation, grant download,
and attachment lifecycle outcomes emit structured `type = "audit_log"` records.
The logs include actor on issuance, workspace, request, operation, grant ID, and
affected object identifiers, but never raw grant tokens, attachment bytes, API
keys, or raw object metadata sidecars.

## Revisions

- 2026-06-11: Extracted remaining story 017 work after verifying scan and
  finalization are implemented.
- 2026-06-11: Replaced database-backed audit events with structured application
  audit logs.
- 2026-06-11: Replaced authenticated attachment-content URLs with short-lived
  Proofplane download grants for human inspection.
- 2026-06-11: Simplified grant use to direct GET with expiry; grants remain
  reusable until expiry to tolerate previews and interrupted downloads.
