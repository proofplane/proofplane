# 003 - PASETO Data-Plane Authentication

**Status:** Done · **Depends on:** 002 · **Spec:** [spec.md](../spec.md#data-plane-authentication-and-authorization)

**Summary** - Build and test the shared `v4.public` verifier,
`ApiTokenContext`, and authorization policy before the external route cutover.
This ticket introduces no dual-authentication behavior.

**Acceptance criteria**

- [x] Given a valid, active token whose user remains a workspace member, when the shared verifier authenticates it, then it returns the expected `ApiTokenContext`.
- [x] Given a verified context without a required permission or for another workspace, when authorization is evaluated, then access is denied without exposing resource existence.
- [x] Given a malformed, forged, expired, revoked, unknown, claim/row-mismatched, or stale-membership token, when the shared verifier authenticates it, then verification fails closed.
- [x] Given existing REST data-plane routes, when this internal foundation ships, then their external authentication behavior is unchanged.

**Tasks**

- [x] Implement API-token verification, custom-claim parsing, exact row/claim matching, and membership revalidation.
- [x] Add `ApiTokenContext` and shared workspace/permission authorization policy without wiring REST routes yet.
- [x] Add best-effort `last_used_at` updates outside authorization correctness.
- [x] Expose the verifier through a shared interface the future MCP runtime can use without depending on REST middleware.
- [x] Add unit and repository-backed tests for success, authorization denial, and every verifier rejection class.

**Notes**

- Revised with the 2026-06-17 spec update: this ticket is an internal
  prerequisite; ticket 004 replaces the actor contract atomically.
- Reconciled the spec on 2026-06-19: `last_used_at` is updated best-effort on
  every successful authentication.
