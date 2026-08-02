# 005 - MCP Upload Preparation

**Status:** Done · **Depends on:** 002, 003, 004, reliability-observability/007 · **Spec:** [spec.md](../spec.md#preparation-contract)

**Summary** - Expose `prepare_evidence_submission_upload` so an authenticated
agent can obtain a one-file HTTP transfer descriptor, submit from its trusted
runtime, and poll the existing submission lifecycle.

**Acceptance criteria**

- [x] Given `write_evidence_submissions` and valid metadata, when preparation is
  called, then it returns the upload ID, preallocated submission ID, PUT
  descriptor, short-lived authorization value, expiry, and maximum size.
- [x] Given missing permission or unavailable evidence, when preparation is
  called, then existing authorization and not-found concealment behavior
  applies and no usable descriptor is returned.
- [x] Given tool arguments and results are inspected, then no file bytes, local
  path, attachment handle, or base64 content is accepted or returned.
- [x] Given the existing human workflow, when the new tool ships, then
  `manage_evidence_submissions` remains available and unchanged.
- [x] Given a completed transfer, when the agent polls
  `get_evidence_submission`, then the existing terminal upload statuses remain
  the processing contract.

**Tasks**

- [x] Add request validation and response schema for the preparation tool.
- [x] Wire the machine grant service into MCP composition.
- [x] Enforce `write_evidence_submissions` and concealed evidence lookup.
- [x] Describe bearer-secret handling and trusted-runtime transfer behavior.
- [x] Add MCP unit and integration tests for success, validation,
  authorization, concealment, and response shape.
- [x] Search modified runtime paths for `.expect(` and remove every occurrence.

**Notes**

- The tool prepares a transfer; it does not read the local file or perform the
  HTTP PUT on behalf of MCP-only clients.
