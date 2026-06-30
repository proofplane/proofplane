# 003 - Guided Agent Connection UI

**Status:** Todo · **Depends on:** 001, 002 · **Spec:** [spec.md](../spec.md#website-experience)

**Summary** - Replace the token-centric MCP preview with a guided setup that
asks which agent the customer uses, launches its best connection path, verifies
authorization, and provides a useful first SOC 2 prompt. All browser-facing
OAuth and connection-management UI is served by the website, not the MCP
server.

**Acceptance criteria**

- [ ] Given a supported client, when a user selects it, then the UI launches or
  explains the native connection path without requiring a terminal or config
  edit.
- [ ] Given completed authorization, when the user returns to Proofplane, then
  the UI records the agent as authorized and points the user back to the
  selected harness for connection health and tool availability.
- [ ] Given an opened install link without completed authorization, when the
  user returns, then the UI does not record an authorized connection.
- [ ] Given an OAuth flow started by an MCP client, when browser UI is needed,
  then the user lands on website/API routes rather than MCP-hosted pages.
- [ ] Given a technical user who needs unattended access, when they choose
  advanced setup, then existing API-token creation remains available and
  clearly separate.

**Tasks**

- [ ] Define the client selection, authorization progress, recovery, and
  revocation states.
- [ ] Replace the incomplete stdio configuration preview.
- [ ] Add connection verification and connection-management API calls.
- [ ] Add client capability and maturity labels.
- [ ] Add first-prompt and advanced-setup content.
- [ ] Add component and browser tests for success, denial, abandonment, and
  unsupported-client states.

**Notes**

- 2026-06-29: Spec now fixes the ownership boundary: OAuth and
  connection-management UI belongs to the website/API surface, not the MCP
  server.
- 2026-06-29: Spec now removes website-owned readiness state. The UI records
  authorization/revocation and leaves MCP connection health to the harness.
