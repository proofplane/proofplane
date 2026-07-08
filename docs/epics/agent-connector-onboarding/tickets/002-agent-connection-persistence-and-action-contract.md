# 002 - Agent Connection Persistence And Action Contract

**Status:** Doing · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#agent-connection-foundation)

**Summary** - Establish the durable agent-connection lifecycle and the
authenticated internal contract used by an Auth0 Redirect Action. This gives
later consent and MCP-runtime work one transactional, replay-safe source of
truth without changing current MCP authorization.

**Acceptance criteria**

- [x] Given a user, client, resource, workspace, canonical permission set, and
  approved continuation, when a pending connection is created, then only
  digests are stored and at most one non-revoked tuple exists.
- [x] Given Auth0 consumes an approved continuation, when the continuation
  validates, then the connection moves from pending to authorized without
  being marked active.
- [x] Given an expired pending connection, when the same tuple is created
  again, then expiry cleanup and replacement happen transactionally.
- [x] Given an authorized or active connection, when reusable lookup runs, then
  it succeeds only for exact subject, client, resource, scopes, and current
  membership.
- [x] Given a denied, replayed, revoked, expired, or membership-less
  connection, when its lifecycle operation runs, then it cannot authorize or
  be reused.
- [x] Given a correctly authenticated Action request, when reusable lookup or
  continuation validation reaches an expected domain outcome, then the route
  returns a tagged `200` result.
- [x] Given malformed input, invalid Action authentication, or a repository
  failure, when an internal route runs, then it returns `400`, `401`, or `500`
  respectively without exposing a secret.
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
- [x] Add validated Action shared-secret configuration.
- [x] Add bearer-protected internal resolve and continuation endpoints with
  tagged outcomes.
- [x] Add migration, repository/service, configuration, black-box route, and
  dedicated repository integration tests.
- [x] Run focused integration tests and `make check`.

**Notes**

- Browser consent, Redirect Action JavaScript and claim injection, MCP runtime
  enforcement, actor-provenance changes, and user-facing revocation are
  explicitly deferred to tickets 003-005.
- 2026-07-05: Split from the former combined workspace-grant and runtime
  authorization ticket; the spec revision records the new delivery boundary.
- 2026-07-05: Implementation and `make check` are complete; status remains
  Doing for delivery review as requested by the ticket split.
- 2026-07-06: PR 40 feedback is tracked in
  [pr-40-review.md](../pr-40-review.md).
- 2026-07-06: The spec now records the shared workspace-permission lookup
  introduced while addressing PR 40 review feedback.
- 2026-07-06: The spec now assigns syntactic validation to exposed Action
  route conversions and defers pending-creation DTO validation to ticket 003.
- 2026-07-06: The spec now records the integration-test boundary: Action
  routes are tested through HTTP, repository lifecycle behavior has a
  dedicated module, and repository setup is limited to otherwise unreachable
  route preconditions.
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
