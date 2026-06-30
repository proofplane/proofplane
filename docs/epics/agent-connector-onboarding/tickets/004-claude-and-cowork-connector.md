# 004 - Claude And Cowork Connector

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#claude-and-cowork)

**Summary** - Make the hosted Proofplane MCP endpoint usable as a remote
connector across Claude and Cowork, then prepare the material required for
reviewed directory distribution. Claude/Cowork is the first-class
non-technical connector path before broader directory or marketplace
submission.

**Acceptance criteria**

- [ ] Given Claude or Cowork custom-connector setup, when the Proofplane
  endpoint is added, then the user can authenticate and use allowed tools.
- [ ] Given a denied or revoked Proofplane grant, when Claude invokes a tool,
  then access fails without falling back to a shared credential.
- [ ] Given the connector is not yet directory-approved, when a customer sets
  it up, then the documented custom-connector path remains usable.
- [ ] Given existing non-Claude MCP clients, when this artifact ships, then
  their protocol behavior is unchanged.

**Tasks**

- [ ] Validate current Claude, Cowork, and Claude Desktop remote-connector
  behavior.
- [ ] Reconcile tool annotations, instructions, approval guidance, and naming.
- [ ] Add a production-like connector smoke test and troubleshooting runbook.
- [ ] Prepare privacy, support, test-account, examples, and directory metadata.
- [ ] Prepare scope descriptions, screenshots, starter prompts, and security
  overview for submission.
- [ ] Document custom-connector setup independently of directory approval.
- [ ] Record host limitations and verified versions in the support matrix.

**Notes**

- 2026-06-29: Spec now sets distribution order: production remote MCP,
  Claude/Cowork custom connector, guided website flow, Codex preview, then
  broader directory submission.
