# MCP Attachment Upload UX

## Principle

The upload page is a utility surface for one job: safely attach evidence bytes
that an MCP agent should not handle. It should be plain, direct, and clear about
what has already been uploaded.

## Page States

- Valid session: show submission attachment inventory, a single file picker,
  and an upload button.
- Successful upload: show only "Upload successful" and tell the human they can
  safely close the page.
- Existing attachment: show the attachment inventory without an upload button.
- Duplicate filename: do not interrupt the human; the server applies a
  macOS-style suffix and the page shows the stored filename.
- Expired or invalid link/session: show "This upload link is no longer
  available" and tell the human to ask the MCP client for a new upload link.
- Upload failure: show a short error and keep the attachment inventory visible.

## First-Pass Controls

Use native browser controls:

- `<input type="file">` for selecting one file;
- one submit button;
- a compact read-only attachment list.

Do not include drag-and-drop, previews, delete actions, download links,
multi-file selection, Auth0 login, or scan-status polling in the first pass.
