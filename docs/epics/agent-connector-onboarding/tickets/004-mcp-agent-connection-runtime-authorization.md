# 004 - MCP Agent Connection Runtime Authorization

**Status:** Todo · **Depends on:** 002, 003 · **Spec:** [spec.md](../spec.md#runtime-authorization)

**Summary** - Bind Auth0 MCP access tokens to the approved Proofplane
connection named by their custom claims and enforce live membership,
revocation, and exact scopes on every protected tool call.

**Acceptance criteria**

- [ ] Given a valid Auth0 access token whose claims match a pending connection,
  when its first protected MCP request runs, then the connection activates
  atomically and the tool uses its workspace authorization.
- [ ] Given a valid token whose claims match an active connection, when a
  protected tool runs, then current membership and the required permission are
  checked and last use is recorded.
- [ ] Given missing or mismatched claims, expiry, membership removal, or local
  revocation, when an existing token is used, then protected access fails
  immediately.
- [ ] Given an agent-backed operation, when audited, then provenance identifies
  the connection without a credential or synthetic API token.
- [ ] Given existing `ppat_`, REST, or ungranted Auth0 callers, when runtime
  authorization ships, then their existing behavior is unchanged.

**Tasks**

- [ ] Parse and validate namespaced connection and workspace claims.
- [ ] Match token subject, client, resource, exact scopes, and workspace to the
  persisted connection.
- [ ] Activate valid pending connections and authorize active connections.
- [ ] Recheck membership, required permissions, revocation, and expiry.
- [ ] Record last use and add agent-connection actor provenance.
- [ ] Add MCP runtime, isolation, expiry, revocation, and compatibility tests.

**Notes**

- Full Auth0 read/write tool support remains required in this ticket.
