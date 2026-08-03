# 001 - Auth0 Harness And Auditor Coverage

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#auth0-harness-and-auditor-coverage)

**Summary** - Restore integration-v2 compilation after custom OTP retirement
and move every auditor story onto the shipped Auth0 hosted-login boundary. Keep
Proofplane transactions, sessions, portal routes, and audit behavior real while
controlling only the external identity-provider outcome.

**Acceptance criteria**

- [x] Given a matching verified Auth0 identity and active invitation, when the
  hosted-login callback completes, then one scoped auditor session opens the
  portal and emits complete secret-free lifecycle audits.
- [x] Given replay, callback mismatch, provider rejection or outage, revoked
  authority, or removed OTP endpoints, when authentication is attempted, then
  no usable auditor session is created and the documented coarse response is
  returned.
- [x] Given existing portal, download, logout, period-filtering, revocation,
  concealment, and escaping stories, when sessions originate through Auth0,
  then those client-visible guarantees remain unchanged.

**Tasks**

- [x] Remove mail configuration, `TestMailAdapter`, and all `proofplane::mailer`
  references from integration-v2 support.
- [x] Add a controllable fake for the auditor identity-provider boundary and
  wire the current `AppDependencies` contract without bypassing HTTP routes.
- [x] Replace OTP send, resend, verification, and delivery-failure helpers and
  stories with explicit login-start and callback arrangements.
- [x] Add focused success, replay, mismatch, unverified identity, provider
  failure, concurrency, and removed-endpoint coverage.
- [x] Reuse Auth0-created cookies across the existing portal and auditor
  download stories without hiding grant or identity setup.
- [x] Run `cargo test --no-run` and the focused integration-v2 auditor modules.

**Notes**

- The retired mailer is not a test dependency to recreate. See the spec's
  [test boundary](../spec.md#test-boundary).
