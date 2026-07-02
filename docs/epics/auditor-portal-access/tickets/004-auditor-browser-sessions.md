# 004 - Auditor Browser Sessions

**Status:** Absorbed by 003 · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#auditor-sessions)

**Summary** - Absorbed into
[003 - Email OTP Verification And Auditor Sessions](./003-email-otp-verification.md)
so OTP verification creates the server-side session directly, without a
temporary verification token.

**Acceptance criteria**

- [ ] Given a valid OTP, when verification succeeds, then Proofplane creates a
  server-side session and sets an HttpOnly, Secure, SameSite cookie.
- [ ] Given a seven-day-old, revoked, missing, or tampered session, when a
  portal route is requested, then access is rejected.
- [ ] Given a grant is revoked, when an existing session is used, then it stops
  working immediately.
- [ ] Given logs and responses, when inspected, then session IDs and cookie
  values are absent.

**Tasks**

- [ ] Add auditor session schema and opaque session ID generation.
- [ ] Add secure cookie creation, lookup, and clearing behavior.
- [ ] Add session helper or middleware for auditor routes.
- [ ] Recheck backing grant on every session lookup.
- [ ] Add logout and expiry handling.
- [ ] Add integration tests for cookie, expiry, revocation, and tampering.
