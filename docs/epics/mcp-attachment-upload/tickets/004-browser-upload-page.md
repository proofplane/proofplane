# 004 - Browser Upload Page

**Status:** Done · **Depends on:** [003](./003-grant-redemption-and-upload-session.md) · **Spec:** [spec.md](../spec.md#page-behavior)

**Summary** - Serve a minimal API-origin upload page that shows existing
attachments and lets the human upload the first file without using MCP for
bytes.

**Acceptance criteria**

- [x] Given a valid upload session, when the page loads, then it shows existing
  attachments with filename, size, and coarse status before any new upload.
- [x] Given the human selects one valid file, when they submit it, then the file
  enters the existing quarantine upload, scan, finalization, and audit pipeline.
- [x] Given an attachment already exists, when the browser posts another file,
  then the signed UI flow returns conflict and does not create a second
  attachment.
- [x] Given upload succeeds, when the browser follows the redirect, then the
  page shows the stored filename and coarse processing status.
- [x] Given the existing authenticated REST upload endpoint receives a duplicate
  filename, when this ships, then its existing behavior remains unchanged.
- [x] Given an existing finalized attachment, when the page renders, then it
  shows a download action scoped to the upload session.

**Tasks**

- [x] Add the server-rendered HTML page and one-file upload form.
- [x] Add grant-session-backed multipart upload that reuses
  `EvidenceSubmissionService`.
- [x] Add server-side first-attachment enforcement for the signed browser flow.
- [x] Render existing attachment inventory and post-upload stored filename.
- [x] Add session-scoped download redirects for finalized attachments.
- [x] Add integration tests for page rendering, upload success, second browser
  upload rejection, expired session behavior, download redirects, and unchanged
  REST duplicate behavior.

**Notes**

- Do not add a React route, SPA asset serving, polling, delete, preview,
  drag-and-drop, or multi-file POST in this ticket.
- Browser uploads compute CRC32C server-side from received bytes; native forms do
  not need to send `Content-Digest`.
- Follow-up commits changed the browser flow from duplicate suffixing to
  first-attachment enforcement and added download redirects for finalized
  attachments; see the spec revision log.
