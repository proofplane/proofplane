# MCP Attachment Upload Spec

## Goal

Let an authenticated MCP client hand a human a short-lived Proofplane URL for
adding attachment bytes to an existing Evidence Submission without passing the
file through chat, model context, or MCP.

The MCP tool creates a narrow delegated browser upload session. Attachment bytes
still enter Proofplane through HTTP and the existing quarantine, scan,
finalization, and audit pipeline.

## Tool Contract

Add the MCP tool:

```text
create_attachment_upload_grant
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
  "intended_use": "human_browser_upload"
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
session is scoped to one workspace and one Evidence Submission, expires 15
minutes after redemption, and is not sliding.

The upload session can:

- render the upload page;
- list existing attachments for the scoped submission;
- upload one file at a time to the scoped submission.

It cannot create submissions, upload to another submission, delete attachments,
download attachments, call arbitrary API routes, or expose the user's API token.

If the upload session expires, the human must ask the agent for a new MCP upload
grant. A new session lists existing attachments before any new upload so the
human can avoid reuploading files.

## HTTP Routes

Add API-origin routes outside normal workspace bearer-token auth:

```text
GET  /evidence-attachment-uploads?token=<grant>
GET  /evidence-attachment-uploads
POST /evidence-attachment-uploads/files
```

The first `GET` redeems the URL token and sets the upload-session cookie. Later
page loads use the cookie. `POST /files` accepts one multipart `file` field and
uses the existing `EvidenceSubmissionService::upload_attachment` and
`create_attachment` flow. Browser uploads compute CRC32C server-side while
streaming; native browser forms do not need to provide `Content-Digest`.

The existing authenticated REST upload endpoint remains unchanged. It continues
to require a bearer API token and keeps its current duplicate-filename behavior.

## Duplicate Filenames

The signed browser upload flow should not fail only because a human chose a
filename that already exists on the submission. For this flow only, the server
renames duplicates before creating the attachment:

```text
report.pdf
report (1).pdf
report (2).pdf
```

The server is the source of truth because concurrent sessions can race. The
implementation may attempt a bounded sequence of suffixes, such as 100 names,
then return conflict if all are unavailable.

The existing filename validation rules still apply after suffixing.

## Page Behavior

The first version is a minimal API-served HTML page, not a React route in the
separate Vite UI app. It may borrow Proofplane visual tokens, but it should not
add new deployment requirements for serving the SPA.

The page allows one file selection and one upload at a time. After success it
shows "Uploaded" and short copy telling the human to ask the MCP client to check
processing status. It also refreshes or displays the current attachment list
with filename, size, and coarse status.

No polling, delete, preview, download, multi-file POST, drag-and-drop, or
product login is part of the first pass.

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
