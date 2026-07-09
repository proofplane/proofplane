# 005 - Connection Management And Guided UI

**Status:** Todo · **Depends on:** 003, 004 · **Spec:** [spec.md](../spec.md#website-experience)

**Summary** - Replace the token-centric MCP preview with a guided setup that
launches each client's verified Proofplane OAuth connection path, hosts the
workspace consent step, and lets users recognize and revoke authorized agent
connections.

**Acceptance criteria**

- [ ] Given a valid Proofplane OAuth transaction, when the user opens the
  workspace step, then the UI displays the verified client, requested scopes,
  and only workspaces the user can currently access.
- [ ] Given a missing, expired, replayed, or invalid OAuth transaction, when
  the consent route opens, then it reveals no connection or workspace data.
- [ ] Given a supported client, when a user selects it, then the UI launches or
  explains the native connection path without requiring a terminal or config
  edit.
- [ ] Given completed authorization, when the user returns to Proofplane, then
  the connection list identifies its workspace, client, scopes, authorization
  time, and last use without claiming the external client is healthy.
- [ ] Given an owned connection, when it is revoked, then it is marked revoked
  locally before remote Auth0 cleanup and unrelated clients and API tokens are
  unchanged.
- [ ] Given an expired 24-hour access token, when the client cannot restart
  OAuth automatically, then the user receives an accurate reconnect path
  instead of a generic tool failure.
- [ ] Given another user's or workspace's connection, when list or revocation
  is attempted, then the request is rejected without revealing its existence.
- [ ] Given a technical user who needs unattended access, when they choose
  advanced setup, then existing API-token creation remains available and
  clearly separate.

**Tasks**

- [ ] Integrate the workspace-consent route into the website shell and add
  polished progress and recovery states.
- [ ] Replace the incomplete stdio configuration preview.
- [ ] Add connection list and revoke API operations.
- [ ] Add client capability and maturity labels.
- [ ] Add authorization status, first-prompt, and advanced-setup content
  without mirroring external client health.
- [ ] Add access-expiry and client-specific reconnect guidance.
- [ ] Add component and browser tests for approval, denial, expiry, replay,
  abandonment, revocation, isolation, and unsupported clients.

**Notes**

- 2026-07-08: Proofplane owns MCP OAuth consent and tokens; Auth0 remains the
  upstream human login provider.
- 2026-07-02: OAuth connections use 24-hour access tokens and may require a
  visible reconnect when the client does not restart authorization.
- The harness remains authoritative for transport health and mounted tools.
