# 002 - MCP Upload Grant Tool

**Status:** Done · **Depends on:** [001](./001-upload-grant-persistence.md) · **Spec:** [spec.md](../spec.md#tool-contract)

**Summary** - Add `create_attachment_upload_grant` so an MCP client can give a
human a short-lived browser URL for uploading attachments to an existing
Evidence Submission.

**Acceptance criteria**

- [x] Given an MCP caller with `WriteEvidenceSubmissions`, when it calls the tool
  with a valid `submission_id`, then the response contains a bearer-secret URL,
  expiry, submission ID, and human-browser intended use.
- [x] Given an MCP caller without write permission or with a missing,
  cross-workspace, or malformed submission ID, when it calls the tool, then it
  receives the appropriate structured MCP error without a URL.
- [x] Given the tool response, when it is returned, then no raw API token,
  upload-session cookie, or file bytes are included.
- [x] Given existing MCP submission and download-grant tools, when this ships,
  then their schemas and behavior remain unchanged.

**Tasks**

- [x] Add the MCP request/response DTOs and schema.
- [x] Authorize with `WriteEvidenceSubmissions`.
- [x] Call the upload grant service and format the compact response.
- [x] Emit `evidence_attachment_upload_grant.issued` audit logs without token or
  URL metadata.
- [x] Add MCP integration tests for success, validation, authorization, and audit
  behavior.

**Notes**

- Keep the tool scoped to `submission_id`; callers can use existing tools to
  find the latest submission before requesting a grant.
