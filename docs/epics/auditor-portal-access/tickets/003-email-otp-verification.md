# 003 - Email OTP Verification And Auditor Sessions

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#otp-verification), [spec.md](../spec.md#auditor-sessions)

**Summary** - Require the browser user opening an invite to prove control of
the configured auditor email, then immediately create a revocable seven-day
server-side auditor session with an HttpOnly cookie.

**Acceptance criteria**

- [x] Given a valid invite, when the auditor requests an OTP, then Proofplane
  sends a single-use code to the grant email.
- [x] Given an expired, reused, wrong, or rate-limited OTP, when verification is
  attempted, then session creation is rejected.
- [x] Given an expired, revoked, missing, or invalid invite, when OTP is
  requested, then no workspace data is exposed.
- [x] Given a valid OTP, when verification succeeds, then Proofplane creates a
  server-side session and sets an HttpOnly, Secure-when-HTTPS, SameSite cookie.
- [x] Given a revoked, missing, tampered, or grant-revoked session, when loaded
  or logged out, then access is rejected or cleared.
- [x] Given logs and responses, when inspected, then invite tokens, OTP codes,
  OTP digests, raw sessions, and cookie values are not exposed.

**Tasks**

- [x] Add a mailer adapter with local/test capture behavior.
- [x] Add OTP schema with digest, expiry, consumed timestamp, and attempt
  tracking.
- [x] Add auditor session schema and opaque session ID generation.
- [x] Add OTP request, verification, and logout routes.
- [x] Emit identifier-only OTP/session audit logs.
- [x] Add integration tests for send, verify, reuse, rate limiting, cookie
  behavior, logout, and revocation.
