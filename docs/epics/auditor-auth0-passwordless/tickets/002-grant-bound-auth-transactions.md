# 002 - Grant-Bound Authentication Transactions

**Status:** Done · **Depends on:** 001 · **Spec:**
[spec.md](../spec.md#authentication-transaction)

**Summary** - Add one-use, grant-bound OIDC transactions and construct Auth0
Authorization Code Flow starts with PKCE, state, nonce, and the invited email
hint. This ensures Auth0 authentication cannot authorize a portal session
without a valid Proofplane invitation.

**Acceptance criteria**

- [x] Given a valid active invitation, when login starts, then Proofplane stores
      digests of random state and nonce plus short-lived PKCE material bound to
      that grant.
- [x] Given the generated authorization redirect, when its parameters are
      inspected, then it contains the exact callback, auditor client, `openid
      email` scope, PKCE S256 challenge, state, nonce, email connection,
      expected login hint, and `prompt=login`.
- [x] Given an invalid, expired, or revoked invitation, when login starts, then
      no transaction is created and no Auth0 redirect is returned.
- [x] Given an expired, consumed, replayed, or cross-grant state, when it is
      claimed, then it is rejected atomically and releases no PKCE material.
- [x] Given existing invitation-token generation and grant authorization, when
      this ticket ships, then their format, expiry, revocation, and audit
      provenance are unchanged.

**Tasks**

- [x] Add the additive `auditor_auth_transactions` migration and constraints.
- [x] Implement CSPRNG state, nonce, and PKCE generation with secret-safe types.
- [x] Persist state and nonce only as SHA-256 digests with ten-minute expiry.
- [x] Implement atomic one-use transaction claim and opportunistic cleanup.
- [x] Build the exact allowlisted Auth0 authorization URL.
- [x] Add repository, service, parameter, expiry, replay, concurrency, and
      secret-exclusion tests.

**Notes**

- `login_hint` is presentation only; authorization still requires callback
  email matching. See [Authorization Start](../spec.md#authorization-start).
- The 2026-07-27 spec revision records digest-based nonce verification and the
  shipped transaction boundary.
