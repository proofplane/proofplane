# 003 - Email OTP Verification

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#otp-verification)

**Summary** - Require the browser user opening an invite to prove control of
the configured auditor email before Proofplane creates any portal session.

**Acceptance criteria**

- [ ] Given a valid invite, when the auditor requests an OTP, then Proofplane
  sends a single-use code to the grant email.
- [ ] Given an expired, reused, wrong, or rate-limited OTP, when verification is
  attempted, then session creation is rejected.
- [ ] Given an expired, revoked, missing, or invalid invite, when OTP is
  requested, then no workspace data is exposed.
- [ ] Given logs and responses, when inspected, then OTP codes and digests are
  not exposed.

**Tasks**

- [ ] Add a mailer adapter with local/test capture behavior.
- [ ] Add OTP schema with digest, expiry, consumed timestamp, and attempt
  tracking.
- [ ] Add OTP request and verification routes.
- [ ] Emit identifier-only OTP audit logs.
- [ ] Add integration tests for send, verify, expiry, reuse, and rate limiting.
