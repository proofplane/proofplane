# 002 - Agent Connection Persistence

**Status:** Doing · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#agent-connection-foundation)

**Summary** - Establish the durable agent-connection lifecycle used by
Proofplane OAuth consent and MCP runtime authorization.

**Acceptance criteria**

- [x] Given a user, client, resource, workspace, canonical permission set, and
  approved continuation, when a pending connection is created, then only
  digests are stored and at most one non-revoked tuple exists.
- [x] Given an approved one-use authorization transition, when it validates,
  then the connection moves from pending to authorized without being marked
  active.
- [x] Given an expired pending connection, when the same tuple is created
  again, then expiry cleanup and replacement happen transactionally.
- [x] Given an authorized or active connection, when reusable lookup runs, then
  it succeeds only for exact subject, client, resource, scopes, and current
  membership.
- [x] Given a denied, replayed, revoked, expired, or membership-less
  connection, when its lifecycle operation runs, then it cannot authorize or
  be reused.
- [x] Given an existing Auth0 principal without a connection, when it
  initializes or lists MCP tools, then those operations still work and
  protected tools remain denied.

**Tasks**

- [x] Add agent connections, normalized permissions, and single-use
  authorization transactions with lifecycle and uniqueness constraints.
- [x] Add domain and repository operations for creation, denial, continuation
  consumption, reusable lookup, activation, use tracking, and revocation.
- [x] Add service policy for expiry replacement, exact-scope reuse, and
  membership checks.
- [x] Add migration, repository/service, and dedicated repository integration
  tests.
- [x] Run focused integration tests and `make check`.

**Notes**

- Browser consent, MCP runtime enforcement, actor-provenance changes, and
  user-facing revocation are explicitly deferred to tickets 003-005.
- 2026-07-05: Split from the former combined workspace-grant and runtime
  authorization ticket; the spec revision records the new delivery boundary.
- 2026-07-05: Implementation and `make check` are complete; status remains
  Doing for delivery review as requested by the ticket split.
- 2026-07-06: PR 40 feedback is tracked in
  [pr-40-review.md](../pr-40-review.md).
- 2026-07-06: The spec now records the shared workspace-permission lookup
  introduced while addressing PR 40 review feedback.
- 2026-07-06: The spec now defers pending-creation DTO validation to ticket
  003.
- 2026-07-06: Repository lifecycle behavior has a dedicated integration
  module.
- 2026-07-06: The spec now makes the connection's pending expiration the sole
  authorization deadline; authorization transactions retain only replay
  protection and consumption state.
- 2026-07-06: The spec now records explicit service policy outcomes for
  continuation consumption and activation, distinct from repository failures;
  repository `Option` results retain conditional row-match semantics.
- 2026-07-07: Continuation consumption now moves a valid row from `pending` to
  `authorized`; ticket 004 remains responsible for `authorized` to `active` on
  first valid MCP use.
- 2026-07-07: Reusable lookup includes both `authorized` and `active`
  connections so retrying Codex login after a completed OAuth flow does not
  create a duplicate pending request.
- 2026-07-06: The spec now records the shared redacted `Sha256Digest` used by
  API-token, continuation, and nonce digests without changing persisted bytes.
- 2026-07-06: The spec now names the generated repository insertion payload
  `NewPendingAgentConnection`, distinct from the persisted connection entity.
