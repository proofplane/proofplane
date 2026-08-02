# 003 - MCP Policy Upload Preparation

**Status:** Done · **Depends on:** 001, 002 · **Spec:** [spec.md](../spec.md#preparation-contract)

**Summary** - Expose `prepare_policy_document_upload` so an authenticated agent
can obtain a one-file HTTP descriptor, transfer from its trusted runtime, and
poll the existing policy document lifecycle.

**Acceptance criteria**

- [x] Given `write_controls`, an eligible policy, and valid metadata, when
  preparation is called, then it returns a persisted upload ID and bounded,
  short-lived PUT descriptor.
- [x] Given missing permission or an unavailable policy, when preparation is
  called, then existing authorization and concealment behavior applies and no
  usable descriptor is returned.
- [x] Given a policy with a current document, when preparation is called, then
  a stable conflict directs the agent to inspect the policy without replacing
  or archiving the document.
- [x] Given tool arguments and results are inspected, then no bytes, local path,
  attachment handle, object key, or base64 content is accepted or returned.
- [x] Given a completed transfer, when the agent calls `get_policy`, then the
  current document and existing upload statuses remain the polling contract.
- [x] Given the existing `manage_policy_document` tool, when this tool ships,
  then the human browser workflow remains available and unchanged.

**Tasks**

- [x] Add request validation and response schema for the preparation tool.
- [x] Wire the policy machine-grant service into MCP composition.
- [x] Enforce `write_controls`, policy concealment, and current-document
  eligibility.
- [x] Register the tool and route it to the policies guide.
- [x] Update policy guidance to distinguish human management from trusted-runtime
  upload and explain `get_policy` polling.
- [x] Add MCP unit and integration tests for schema, success, validation,
  authorization, concealment, conflict, and response shape.
- [x] Search modified runtime paths for `.expect(` and remove every occurrence.

**Notes**

- The tool prepares a transfer but never reads a local file or performs the PUT
  for an MCP-only client.
