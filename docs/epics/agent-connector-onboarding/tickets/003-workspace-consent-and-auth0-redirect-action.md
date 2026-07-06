# 003 - Workspace Consent And Auth0 Redirect Action

**Status:** Todo · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#workspace-grant-bridge)

**Summary** - Add the Auth0 Redirect Action and minimal secure browser consent
step that turns a verified authorization transaction into an approved pending
connection and namespaced token claims.

**Acceptance criteria**

- [ ] Given a visible Auth0 MCP authorization, when the Action runs, then the
  user can approve one currently accessible workspace and Auth0 receives the
  connection and workspace claims.
- [ ] Given an exact reusable connection, when authorization repeats, then the
  Action injects its claims without another workspace redirect.
- [ ] Given no reusable connection during `prompt=none`, when the Action runs,
  then it returns `interaction_required`.
- [ ] Given tampered, expired, replayed, wrong-state, wrong-client, denied, or
  inaccessible-workspace consent, when handled, then no usable pending
  connection or credential is created.
- [ ] Given a normal website session without a valid Action transaction, when
  consent is attempted, then it cannot mint a connection.

**Tasks**

- [ ] Implement the post-login Redirect Action and namespaced claim injection.
- [ ] Add signed, short-lived transaction input and continuation output.
- [ ] Add a minimal API-served workspace consent page with approve and deny.
- [ ] Recheck subject, client, resource, scopes, state, and membership on
  approval.
- [ ] Connect approval and denial to ticket 002 lifecycle operations.
- [ ] Add Action, browser-route, tamper, replay, expiry, and denial tests.

**Notes**

- Polished React integration remains ticket 005.
- Auth0 Action deployment automation is not part of this ticket.
