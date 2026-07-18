# 005 - Policy Document Management Page

**Status:** Done · **Depends on:** [004](./004-mcp-policy-document-grants.md) · **Spec:** [ux.md](../ux.md#policy-document-management)

**Summary** - Add an evidence-like server-rendered page where the delegated
human can upload, inspect, download, delete, and reupload the policy's single
document.

**Acceptance criteria**

- [x] Given a valid policy document session with no current document, when
  the page opens and one valid file is submitted, then the file enters the
  policy document lifecycle and the redirected page shows its status.
- [x] Given a current document, when the page renders, then upload is
  unavailable, terminal documents can be archived, and only `uploaded`
  documents can be downloaded.
- [x] Given a terminal document is archived, when the redirected page opens,
  then the document is hidden and a new upload is available.
- [x] Given an expired, invalid, archived-policy, wrong-scope, or missing
  session/resource, when a browser action is attempted, then generic
  unavailable UI appears without workspace data leakage.
- [x] Given the existing evidence management page, when the policy page ships,
  then its behavior and styling remain unchanged while the two pages remain
  visibly consistent.

**Tasks**

- [x] Add policy-scoped inventory, multipart upload, archive, and download
  routes that recheck session and resource eligibility.
- [x] Build the server-rendered page from the existing evidence document
  visual and accessibility patterns.
- [x] Enforce one-file upload, terminal-only archive, uploaded-only download,
  and post/redirect/get behavior.
- [x] Preserve safe download headers and secret/object-key logging exclusions.
- [x] Emit identifier-only browser mutation and download audit events.
- [x] Add HTTP integration tests for page states, escaping, upload, archive,
  reupload, download, expiry, and responsive/accessibility hooks.
