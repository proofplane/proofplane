# MCP Attachment Management Spec

## Goal

Let an authenticated MCP client hand a human a short-lived Proofplane URL for
adding or downloading attachment bytes for an existing Evidence Submission
without passing the file through chat, model context, or MCP.

The MCP tool creates a narrow delegated browser attachment-management session.
Uploaded attachment bytes still enter Proofplane through HTTP and the existing
quarantine, scan, finalization, and audit pipeline.

## Tool Contract

Add the MCP tool:

```text
manage_evidence_submission_attachment
```

Input:

```json
{
  "submission_id": "uuid"
}
```

The tool requires `WriteEvidenceSubmissions` for the caller's API token and
verifies that the submission exists in the token workspace. It does not accept a
file, file content, workspace ID, evidence request ID, API token, or raw upload
credential.

Response:

```json
{
  "url": "https://api.example/evidence-attachment-uploads?token=...",
  "expires_at": "2026-06-28T12:00:00.000Z",
  "submission_id": "uuid",
  "url_secret_type": "bearer_secret",
  "intended_use": "human_browser_attachment_management"
}
```

The URL is a bearer secret for human presentation. Agents must not fetch,
summarize, log, or persist it. Attachment bytes never pass through MCP or model
context.

## Upload Grant Model

Upload grants are persisted because the URL is single-use. A grant records:

- grant ID;
- workspace ID;
- evidence submission ID;
- issuing user ID;
- issuing API token ID;
- issued time;
- five-minute URL expiry;
- optional redeemed time.

The URL token is encrypted and authenticated using a dedicated
`paseto.upload_grant` PASETO `v4.local` keyring with upload-specific audience
and implicit assertion. It carries enough information to identify the grant and
validate issuer, audience, expiry, and version, but redemption still checks and
marks the database row in one atomic operation.

Malformed, expired, already redeemed, cross-workspace, missing, and
authorization-invalid grants all resolve to a generic unavailable result. The
server must not reveal whether a workspace or submission exists through a
bearer link.

## Browser Session Model

Opening the grant URL consumes the URL once and establishes a separate
HttpOnly, SameSite upload-session cookie scoped to the upload routes. The
session is scoped to one workspace and one Evidence Submission, expires at the
grant URL's original `expires_at`, and is not sliding.

The upload session can:

- render the upload page;
- list existing attachments for the scoped submission;
- upload the first file for the scoped submission when none exists;
- issue a download redirect for a finalized attachment in the scoped submission.

It cannot create submissions, upload to another submission, delete attachments,
call arbitrary API routes, or expose the user's API token.

If the upload session expires, the human must ask the agent for a new MCP upload
grant. A new session lists existing attachments before any new upload so the
human can avoid reuploading files.

## HTTP Routes

Add API-origin routes outside normal workspace bearer-token auth:

```text
GET  /evidence-attachment-uploads?token=<grant>
GET  /evidence-attachment-uploads
POST /evidence-attachment-uploads/files
GET  /evidence-attachment-uploads/files/{attachment_id}/download
```

The first `GET` redeems the URL token, sets the upload-session cookie, and
redirects to `/evidence-attachment-uploads` so refreshes use the cookie instead
of the single-use token. Later page loads use the cookie. `POST /files` accepts
one multipart `file` field and uses the existing
`EvidenceSubmissionService::upload_attachment` and first-attachment creation
flow. Browser uploads compute CRC32C server-side while streaming; native browser
forms do not need to provide `Content-Digest`.

The download route verifies the upload-session cookie, issues the existing
short-lived attachment download grant for a finalized attachment in the scoped
submission, audits that issuance, and redirects the browser to the grant URL.

The existing authenticated REST upload endpoint remains unchanged. It continues
to require a bearer API token and keeps its current duplicate-filename behavior.

## First Attachment Only

The signed browser upload flow accepts only the first attachment for the scoped
submission. If an attachment already exists, the page shows the existing
inventory without an upload form and `POST /files` returns conflict without
creating another object. Concurrent browser uploads race through the same
first-attachment insert path, so only one attachment row is created.

The existing authenticated REST upload endpoint remains multi-attachment and
keeps its current duplicate-filename behavior.

## Page Behavior

The first version is a minimal API-served HTML page, not a React route in the
separate Vite UI app. It may borrow Proofplane visual tokens, but it should not
add new deployment requirements for serving the SPA.

The page allows one file selection and one upload total for the scoped
submission. After success it returns to the attachment-management page and shows
the stored file with filename, size, and coarse status. Later visits display the
same existing attachment list without an upload button. Uploaded attachments show
a download button; processing or failed attachments do not.

No polling, delete, preview, multi-file POST, drag-and-drop, or product login is
part of the first pass.

## Audit And Logging

Grant issuance emits `evidence_attachment_upload_grant.issued` with client type
`mcp`, user ID, API token ID, workspace ID, submission ID, operation name, and
request/session correlation. Raw grant tokens and URLs are never logged or
stored in audit metadata.

Successful browser uploads continue to emit attachment acceptance audit events,
with an operation that distinguishes grant-backed browser upload from the
existing authenticated REST upload if useful.

Application tracing and ingress logs must not record query-token values.

## Revisions

- 2026-06-28: Initial scope for MCP-issued single-use browser upload grants,
  grant-authenticated attachment upload, existing attachment visibility, and
  duplicate filename suffixing for the signed upload UI only.
- 2026-06-29: Ticket 001 implementation uses a dedicated `paseto.upload_grant`
  keyring for single-use upload grant URL tokens.
- 2026-06-29: Ticket 003 ships grant redemption plus session-backed JSON
  inventory; HTML rendering and `POST /files` remain ticket 004.
- 2026-06-29: Ticket 004 ships the API-served HTML page and native browser
  upload route. Browser uploads compute CRC32C server-side; authenticated REST
  uploads keep requiring client-provided `Content-Digest`.
- 2026-06-29: Follow-up implementation redirects after browser POST so refreshes
  do not resubmit, bounds the upload-session cookie to the grant expiry, limits
  the browser flow to the first attachment only, and adds session-scoped
  download redirects for finalized attachments.
