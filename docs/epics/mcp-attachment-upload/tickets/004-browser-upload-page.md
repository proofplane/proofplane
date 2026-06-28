# 004 - Browser Upload Page

**Status:** Todo · **Depends on:** [003](./003-grant-redemption-and-upload-session.md) · **Spec:** [spec.md](../spec.md#page-behavior)

**Summary** - Serve a minimal API-origin upload page that shows existing
attachments and lets the human upload one additional file at a time without
using MCP for bytes.

**Acceptance criteria**

- [ ] Given a valid upload session, when the page loads, then it shows existing
  attachments with filename, size, and coarse status before any new upload.
- [ ] Given the human selects one valid file, when they submit it, then the file
  enters the existing quarantine upload, scan, finalization, and audit pipeline.
- [ ] Given the selected filename already exists, when the upload is accepted,
  then the signed UI flow stores a macOS-style suffixed filename instead of
  failing on the duplicate.
- [ ] Given upload succeeds, when the page updates, then it says "Uploaded" and
  tells the human to ask the MCP client to check processing status.
- [ ] Given the existing authenticated REST upload endpoint receives a duplicate
  filename, when this ships, then its duplicate-error behavior remains
  unchanged.

**Tasks**

- [ ] Add the server-rendered HTML page and one-file upload form.
- [ ] Add grant-session-backed multipart upload that reuses
  `EvidenceSubmissionService`.
- [ ] Add server-side duplicate filename suffixing for the signed browser flow.
- [ ] Render existing attachment inventory and post-upload stored filename.
- [ ] Add integration tests for page rendering, upload success, duplicate
  suffixing, expired session behavior, and unchanged REST duplicate behavior.

**Notes**

- Do not add a React route, SPA asset serving, polling, delete, preview,
  download, drag-and-drop, or multi-file POST in this ticket.
