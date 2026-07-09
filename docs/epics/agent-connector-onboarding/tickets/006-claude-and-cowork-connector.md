# 006 - Claude And Cowork Connector

**Status:** Todo · **Depends on:** 003, 004 · **Spec:** [spec.md](../spec.md#claude-and-cowork)

**Summary** - Validate that Claude and Cowork can use the hosted Proofplane MCP
endpoint through Proofplane OAuth discovery, DCR or manual registration, and
workspace consent. This ticket no longer creates Proofplane OAuth client
plumbing; it verifies host behavior, reconnect behavior, and any distribution
material needed for a first-class customer path.

**Acceptance criteria**

- [ ] Given Claude or Cowork custom-connector setup, when the Proofplane
  endpoint is added, then the client discovers Auth0, completes workspace
  consent, and uses only allowed tools.
- [ ] Given a denied or revoked Proofplane grant, when Claude invokes a tool,
  then access fails without falling back to a shared credential.
- [ ] Given the 24-hour access token expires, when Claude or Cowork next
  invokes Proofplane, then automatic reauthorization or the exact user-visible
  reconnect behavior is verified and documented.
- [ ] Given the connector is not yet directory-approved, when a customer sets
  it up, then the documented custom-connector path remains usable.
- [ ] Given existing non-Claude MCP clients, when this artifact ships, then
  their protocol behavior is unchanged.

**Tasks**

- [ ] Validate current Claude, Cowork, and Claude Desktop remote-connector
  behavior with Auth0 DCR where available and manual registration where DCR is
  unavailable.
- [ ] Reconcile tool annotations, instructions, approval guidance, and naming.
- [ ] Add a production-like connector smoke test and troubleshooting runbook.
- [ ] Test token expiry with and without an active Auth0 browser session.
- [ ] Prepare privacy, support, test-account, examples, and directory metadata
  only if the chosen Claude/Cowork path requires reviewed distribution.
- [ ] Prepare scope descriptions, screenshots, starter prompts, and security
  overview for submission only if the chosen Claude/Cowork path requires them.
- [ ] Document custom-connector setup independently of directory approval.
- [ ] Record host limitations and verified versions in the support matrix.

**Notes**

- 2026-07-08: The spec now requires Claude/Cowork validation against
  Proofplane discovery, client registration, and workspace consent.
- 2026-07-02: The support matrix must record behavior after an access token
  expires because the initial release does not request `offline_access`.
- 2026-07-07: Codex DCR validation removed the need for Proofplane-side static
  client allowlisting. Claude/Cowork still needs host-specific validation
  because DCR support, callback behavior, and token-expiry recovery are client
  capabilities.
- 2026-06-29: Spec now sets distribution order: production remote MCP,
  Claude/Cowork custom connector, guided website flow, Codex preview, then
  broader directory submission.
