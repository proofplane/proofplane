# 002 - Grant-Bound Authentication Transactions

**Status:** Todo · **Depends on:** 001 · **Spec:**
[spec.md](../spec.md#authentication-transaction)

**Summary** - Add one-use, grant-bound OIDC transactions and construct Auth0
Authorization Code Flow starts with PKCE, state, nonce, and the invited email
hint. This ensures Auth0 authentication cannot authorize a portal session
without a valid Proofplane invitation.

**Acceptance criteria**

- [ ] Given a valid active invitation, when login starts, then Proofplane stores
      digests of random state and nonce plus short-lived PKCE material bound to
      that grant.
- [ ] Given the generated authorization redirect, when its parameters are
      inspected, then it contains the exact callback, auditor client, `openid
      email` scope, PKCE S256 challenge, state, nonce, email connection,
      expected login hint, and `prompt=login`.
- [ ] Given an invalid, expired, or revoked invitation, when login starts, then
      no transaction is created and no Auth0 redirect is returned.
- [ ] Given an expired, consumed, replayed, or cross-grant state, when it is
      claimed, then it is rejected atomically and releases no PKCE material.
- [ ] Given existing invitation-token generation and grant authorization, when
      this ticket ships, then their format, expiry, revocation, and audit
      provenance are unchanged.

**Tasks**

- [ ] Add the additive `auditor_auth_transactions` migration and constraints.
- [ ] Implement CSPRNG state, nonce, and PKCE generation with secret-safe types.
- [ ] Persist state and nonce only as SHA-256 digests with ten-minute expiry.
- [ ] Implement atomic one-use transaction claim and opportunistic cleanup.
- [ ] Build the exact allowlisted Auth0 authorization URL.
- [ ] Add repository, service, parameter, expiry, replay, concurrency, and
      secret-exclusion tests.

**Notes**

- `login_hint` is presentation only; authorization still requires callback
  email matching. See [Authorization Start](../spec.md#authorization-start).
