# 002 - Connection Lifecycle And Audit

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#connection-lifecycle-and-audit)

**Summary** - Give users a recognizable list of authorized agent connections
and let them revoke one client without disturbing unrelated clients or API
tokens.

**Acceptance criteria**

- [ ] Given several authorized clients, when a user lists connections, then
  each entry identifies its workspace, client, granted access, and lifecycle
  timestamps without exposing credentials.
- [ ] Given authenticated MCP traffic reaches Proofplane, when the connection
  is listed, then `last_used_at` is available as audit/debug metadata without
  being treated as a readiness gate.
- [ ] Given an owned connection, when the user revokes it, then future refresh
  and MCP requests through that connection fail.
- [ ] Given another user's or workspace's connection, when access or revocation
  is attempted, then the request is rejected without revealing its existence.
- [ ] Given existing API tokens, when a client connection is revoked, then
  those unrelated tokens remain valid.

**Tasks**

- [ ] Add OAuth client, agent connection, and credential-digest persistence.
- [ ] Add management-plane list and revoke operations.
- [ ] Maintain authorization, revocation, refresh, and last-used metadata.
- [ ] Record authorization, refresh, rejection, use, and revocation audit
  events.
- [ ] Avoid storing raw credentials where digests, encryption, or provider
  references suffice.
- [ ] Add repository, service, route, isolation, and audit tests.
- [ ] Reconcile connection lifecycle details into the spec.

**Notes**

- 2026-06-29: Spec now defines the logical persisted model and removes
  website-owned readiness state.
