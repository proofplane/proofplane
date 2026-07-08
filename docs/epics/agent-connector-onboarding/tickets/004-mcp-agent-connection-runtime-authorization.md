# 004 - MCP Agent Connection Runtime Authorization

**Status:** Done · **Depends on:** 002, 003 · **Spec:** [spec.md](../spec.md#runtime-authorization)

**Summary** - Bind Proofplane PASETO MCP access tokens to the approved
Proofplane connection named by their claims and enforce live membership,
revocation, and exact scopes on every protected tool call.

**Acceptance criteria**

- [x] Given a valid Proofplane PASETO access token whose claims match an
  authorized connection, when its first protected MCP request runs, then the
  connection activates atomically and the tool uses its workspace authorization.
- [x] Given a valid token whose claims match an active connection, when a
  protected tool runs, then current membership and the required permission are
  checked and last use is recorded.
- [x] Given missing or mismatched claims, expiry, membership removal, or local
  revocation, when an existing token is used, then protected access fails
  immediately.
- [x] Given an agent-backed operation, when audited, then provenance identifies
  the connection without a credential or synthetic API token.
- [x] Given existing `ppat_`, REST, or ungranted OAuth callers, when runtime
  authorization ships, then their existing behavior is unchanged.

**Tasks**

- [x] Parse and validate Proofplane MCP token connection and workspace claims.
- [x] Match token subject, client, resource, exact scopes, and workspace to the
  persisted connection.
- [x] Activate valid authorized connections and authorize active connections.
- [x] Recheck membership, required permissions, revocation, and expiry.
- [x] Record last use and add agent-connection actor provenance.
- [x] Add MCP runtime, isolation, expiry, revocation, and compatibility tests.

**Notes**

- Implemented with agent-connection attribution for MCP-created submissions and
  attachment upload grants; see the 2026-07-07 spec revision.
