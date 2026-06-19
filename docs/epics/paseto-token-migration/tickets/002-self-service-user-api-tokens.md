# 002 - Self-Service User API Tokens

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#user-api-tokens)

**Summary** - Let an Auth0-authenticated workspace member issue, list, and revoke
their own workspace-scoped `v4.public` API tokens with explicit permissions and
required but unbounded future expiration. Raw tokens are shown once; persisted
records support lifecycle, audit attribution, and immediate revocation.

**Acceptance criteria**

- [x] Given a current workspace member, when they issue a token with valid permissions and a future expiration, then the response shows the raw PASETO once as `api_token` and stores only its lifecycle metadata.
- [x] Given an expiration arbitrarily far in the future, when the timestamp is valid, then issuance succeeds without applying a maximum token lifetime.
- [x] Given a user listing their tokens, when a token exists, then name, workspace, permissions, expiration, revocation, last-use, and creation metadata are returned without raw tokens or key material.
- [x] Given a token owned by the caller in the path workspace, when it is revoked twice, then both requests succeed without deleting its historical record.
- [x] Given a non-member, another user's token ID, an invalid permission, a missing expiration, or an expiration that is not in the future, when management is attempted, then the request is rejected without leaking token or workspace existence.
- [x] Given existing data-plane routes, when token management ships, then route authentication remains unchanged until the atomic cutover ticket.

**Tasks**

- [x] Add `api_tokens` with required `expires_at` and `api_token_permissions` using the existing `WorkspacePermission` values and database constraint in a forward `V005` migration.
- [x] Add API-token domain types and repository operations for issue, list, lookup, and idempotent revoke.
- [x] Add Auth0-protected `POST`/`GET`/`DELETE /workspaces/{workspace_id}/api-tokens` routes scoped to the current user.
- [x] Mint immutable `v4.public` claims with required issuer, audience, subject, token ID, workspace, permissions, version, and expiration without a maximum TTL.
- [x] Return the raw token only from create as `api_token` and redact all token/key material from errors and logs.
- [x] Add unit and integration tests for success, one-time disclosure, ownership isolation, permission reuse, missing/past/far-future expiration, expiry, and revocation.

**Notes**

- Revised with the 2026-06-17 spec update: expiration is optional and token
  permissions reuse `WorkspacePermission` rather than defining a parallel enum.
- Revised again on 2026-06-17: expiration is required, but any valid future
  timestamp is accepted without a maximum TTL.
- 2026-06-18 implementation note: this ticket uses `V005`; final `V001`
  consolidation remains deferred to ticket 004.
- The 2026-06-19 spec revision preserves this management and lifecycle model
  but supersedes PASETO issuance; ticket 006 adds compact opaque issuance and
  digest persistence.
