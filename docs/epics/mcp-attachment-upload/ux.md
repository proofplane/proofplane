# MCP Attachment Management UX

## Principle

The management page is a utility surface for one job: safely attach or download
evidence bytes that an MCP agent should not handle. It should be plain, direct,
and clear about what has already been uploaded.

## Page States

- Valid session: show submission attachment inventory, a single file picker,
  and an upload button.
- Successful upload: return to the management page and show the uploaded
  filename, size, and status.
- Existing attachment: show the attachment inventory without an upload button.
- Finalized attachment: show a download button.
- Processing or failed attachment: show the status without a download button.
- Duplicate filename: do not interrupt the human; the server applies a
  macOS-style suffix and the page shows the stored filename.
- Expired or invalid link/session: show "This upload link is no longer
  available" and tell the human to ask the MCP client for a new upload link.
- Upload failure: show a short error and keep the attachment inventory visible.

## First-Pass Controls

Use native browser controls:

- `<input type="file">` for selecting one file;
- one submit button;
- a compact attachment list with download links for finalized files.

Do not include drag-and-drop, previews, delete actions, multi-file selection,
Auth0 login, or scan-status polling in the first pass.
