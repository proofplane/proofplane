# Evidence Lifecycle Completion Spec

## Goal

Complete the evidence lifecycle on top of the implemented submission,
quarantine, ClamAV scan, and finalization pipeline. Callers must be able to find
the latest submission, create a safe human download grant for finalized
attachment bytes, and exercise the flow from seeded local data.

## Current Implementation

- Submission create and detail APIs are implemented.
- Multipart uploads require CRC32C, enforce portable attachment filenames, and
  write to filesystem quarantine storage.
- Attachment creation and `attachment.scan_requested` enqueue atomically.
- ClamAV scanning and finalization are idempotent worker deliveries.
- The latest-submission route and repository query are implemented and tested.
- Stateless attachment download grants are implemented and tested.
- There is no seeded submission/object.

## API Contract

Add:

```text
GET /workspaces/{workspace_id}/evidence-requests/{evidence_request_id}/submissions/latest
POST /workspaces/{workspace_id}/evidence-submissions/{submission_id}/attachments/{attachment_id}/download-grants
GET /attachment-downloads?token=<JWT>
```

The latest endpoint returns the existing submission-detail shape. It orders by
`received_at DESC, id DESC` and returns `404` when the Evidence Request is
missing, belongs to another workspace, or has no submissions.

The grant endpoint authenticates and authorizes the workspace actor for evidence
submission reads, verifies attachment eligibility, and returns an HTTPS URL
containing a signed JWT intended for human inspection. The grant is scoped to
one attachment and expires after five minutes. It may be downloaded more than
once before expiry so browser retries, link previews, and interrupted transfers
do not permanently invalidate it. The response includes expiry, filename,
content type, and content length, but not the object key or storage backend.

The download route is outside workspace-authenticated routes. Possession of the
JWT is the authorization, so the URL is a short-lived bearer secret. A
valid GET streams bytes immediately with the stored content type, a safe
`Content-Disposition` filename, `Cache-Control: private, no-store`, and
`Referrer-Policy: no-referrer`.

Multipart upload filenames are preserved exactly when valid. They must be
non-blank portable ASCII names of at most 255 UTF-8 bytes, using only letters,
digits, spaces, `.`, `_`, `-`, `(`, and `)`. Path separators, quotes, Unicode,
control characters, and the exact names `.` and `..` are rejected with `400
bad_request`. Leading dots and spaces and trailing spaces remain valid. Because
persisted names satisfy this contract, downloads emit
`Content-Disposition: attachment; filename="<filename>"` without fallback or
RFC 5987 encoding.

Expired, malformed, tampered, unsupported-version, malicious, failed, missing,
cross-submission, and cross-workspace grants return `404`. Pending and
finalizing attachments return `409 attachment_not_ready` at grant creation; no
grant is issued. Every GET rechecks the current attachment and object metadata,
so an attachment that later becomes ineligible stops downloading even before
the grant expires.

## Download Grant Model

Grants are stateless HS256-signed JWTs. No grant row, token hash, consumption
state, or individual revocation record is persisted. The configured signing
secret is base64 encoded, redacted by the configuration type, and must decode to
at least 32 bytes. Single-key rotation may invalidate URLs issued during the
preceding five minutes.

The protected JWT header must select HS256. Registered claims are:

- `iss`: the configured public API origin;
- `aud`: `proofplane-attachment-download`;
- `jti`: a generated grant UUID;
- `iat`: issuance time;
- `exp`: issuance plus five minutes.

Custom claims carry token version `1` and the workspace, submission, attachment,
and issuing actor UUIDs. Claims identify the issuer but remain readable to
anyone holding the URL. Redemption requires a valid signature, expected
issuer/audience, supported version, complete UUID claims, and an unexpired
five-minute lifetime.

The URL is constructed from `server.public_api_base_url` as
`/attachment-downloads?token=<JWT>`. Production origins require HTTPS; loopback
HTTP is allowed for local development. Application tracing records only the
matched route path and never query parameters.

The `token` query parameter is a bearer secret. Production ingress access logs,
analytics, browser monitoring, and error-reporting systems must redact or omit
it. Referrer-bearing pages must not receive the URL. Browser history,
link-preview systems, and endpoint security software may still observe it; the
five-minute expiry limits the value of that exposure.

Grant creation proves only that an authorized workspace actor requested human
access. Download GETs do not identify the human opening the URL. Human-level
attribution would require an interactive login and is explicitly not part of
this minimal browserless flow.

## Retrieval Invariants

The repository loads a candidate only when all signed identifiers join through
one workspace, submission, attachment, and existing issuing actor. Grant
creation requires:

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

## Revisions

- 2026-06-11: Extracted remaining evidence lifecycle work after verifying scan and
  finalization are implemented.
- 2026-06-11: Replaced database-backed audit events with structured application
  audit logs.
- 2026-06-11: Replaced authenticated attachment-content URLs with short-lived
  Proofplane download grants for human inspection.
- 2026-06-11: Simplified grant use to direct GET with expiry; grants remain
  reusable until expiry to tolerate previews and interrupted downloads.
- 2026-06-14: Replaced persisted opaque grants with stateless, five-minute
  HS256 JWT query-parameter URLs and made external token redaction mandatory.
- 2026-06-15: Restricted attachment upload filenames to a portable ASCII subset
  so downloads can use the stored name directly in `Content-Disposition`.
- 2026-06-16: Moved evidence lifecycle audit logging ownership to the
  Reliability and Observability epic.
