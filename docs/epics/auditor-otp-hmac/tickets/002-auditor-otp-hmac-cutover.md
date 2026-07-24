# 002 - Auditor OTP HMAC Cutover

**Status:** Todo · **Depends on:** 001 · **Spec:**
[spec.md](../spec.md#persistence-and-runtime-flow)

**Summary** - Cut auditor OTP issuance and verification over to the keyed
digest component, persisting the non-secret key ID needed for safe rotation.
Remove the offline-enumerable plain SHA-256 representation without changing the
auditor login or email experience.

**Acceptance criteria**

- [ ] Given a newly requested OTP, when its row is inspected, then it contains
      a 32-byte HMAC tag and active key ID but not the plaintext code or an
      unkeyed OTP digest.
- [ ] Given the correct code and recorded key, when verification runs, then the
      OTP is atomically consumed and the existing auditor session is created.
- [ ] Given a wrong code, malformed code, expired OTP, consumed OTP, or unknown
      recorded key, when verification runs, then access fails closed without
      exposing secrets or creating a session.
- [ ] Given key rotation with an old key retained, when an old in-flight OTP
      and a newly issued OTP are verified, then each uses its recorded key and
      both succeed.
- [ ] Given mail delivery fails, when OTP issuance rolls back logically, then
      the new row is deleted and the previous valid code and rate-limit
      capacity are preserved.
- [ ] Given existing auditor and session behavior, when this ships, then expiry,
      attempt limits, send limits, Resend idempotency, HTTP responses, cookies,
      and session-token digests are unchanged.

**Tasks**

- [ ] Add required `digest_key_id` persistence and its schema constraints.
- [ ] Store the active key ID and HMAC tag during OTP issuance.
- [ ] Resolve the recorded key and verify tags in constant time.
- [ ] Preserve atomic consume/session creation and wrong-attempt accounting.
- [ ] Preserve failed-mail cleanup and sanitized error mapping.
- [ ] Add concrete-Postgres coverage for persistence, verification, rotation,
      rejection, cleanup, and adjacent unchanged behavior.
- [ ] Document local reset/reseed and production additive-migration handling.

**Notes**

- Do not add a legacy `SHA256(code)` fallback; see
  [Persistence And Runtime Flow](../spec.md#persistence-and-runtime-flow).
