# 003 - Proofplane OAuth Workspace Consent

**Status:** Doing · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#workspace-grant-bridge)

**Summary** - Add the Proofplane-hosted OAuth consent step that follows
upstream Auth0 login, selects one workspace, creates an authorized connection,
and returns a one-use authorization code to the MCP client.

**Acceptance criteria**

- [x] Given a visible Proofplane MCP authorization, when upstream Auth0 login
  completes, then the user can approve one currently accessible workspace and
  Proofplane issues an authorization code for that approved connection.
- [ ] Given an exact reusable connection, when authorization repeats, then the
  Action injects its claims without another workspace redirect.
- [ ] Given no reusable connection during non-interactive authorization, when
  Proofplane would need workspace selection, then it returns an OAuth error
  rather than minting a credential.
- [x] Given tampered, expired, replayed, wrong-state, wrong-client, denied, or
  inaccessible-workspace consent, when handled, then no usable pending
  connection or credential is created.
- [x] Given a normal website session without a valid Action transaction, when
  consent is attempted, then it cannot mint a connection.

**Tasks**

- [x] Implement Proofplane `/oauth/authorize`, `/oauth/auth0/callback`, and
  `/oauth/consent` consent handling.
- [x] Add short-lived authorization request and one-use code persistence.
- [x] Add a minimal API-served workspace consent page with approve and deny.
- [x] Validate all pending-creation request fields in the consent route with
  an accumulating `validate!` DTO conversion before calling the service.
- [x] Recheck subject, client, resource, scopes, state, and membership on
  approval.
- [x] Connect approval and denial to ticket 002 lifecycle operations and local
  OAuth code issuance.
- [x] Add Action, browser-route, tamper, replay, expiry, and denial tests.

**Notes**

- Polished React integration remains ticket 005.
- Auth0 Redirect Action deployment is superseded by the Proofplane OAuth
  facade. Auth0 remains the upstream login app only.
- Ticket 002 deliberately exposes no pending-creation endpoint; syntactic
  pending-payload validation belongs to this ticket's consent route.
- Proofplane DCR and consent are the supported development path. Denial,
  silent-auth, and reusable-connection smoke tests remain before this ticket
  can move to Done. Activation on first MCP use remains ticket 004.
- 2026-07-07: Codex completed the real DCR authorization path after restarting
  the app, confirming the earlier callback refusal was a local callback-listener
  lifecycle issue rather than a Proofplane or Auth0 transaction failure.
- 2026-07-06: The spec was reconciled with the shipped redirect-token,
  denial-without-persistence, and native `prompt=none` contracts.
- 2026-07-06: Manual development-tenant verification is documented in
  [003-local-auth0-mcp-consent.md](../runbooks/003-local-auth0-mcp-consent.md).
