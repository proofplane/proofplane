# 001 - OTP HMAC Keyring

**Status:** Todo · **Depends on:** none · **Spec:**
[spec.md](../spec.md#key-configuration-and-rotation)

**Summary** - Add a validated, rotation-aware OTP HMAC keyring and one
domain-separated component for producing and verifying grant-bound tags. This
creates the security primitive required for the persistence cutover without
changing live OTP behavior yet.

**Acceptance criteria**

- [ ] Given valid keyring configuration, when the application loads it, then
      exactly one active 32-byte key is available and all secret output is
      redacted.
- [ ] Given missing, blank, malformed, duplicate, or unresolved key
      configuration, when startup loads configuration, then validation fails
      with field-specific errors and no secret value is exposed.
- [ ] Given a valid grant ID and six-digit code, when a tag is produced and
      verified, then it matches the fixed HMAC-SHA-256 vector and verification
      uses the constant-time MAC API.
- [ ] Given a different grant, code, or key, when verification runs, then the
      tag is rejected; malformed codes and unknown key IDs also fail closed.
- [ ] Given existing session-token behavior, when this ticket ships, then its
      SHA-256 digest format and active-session compatibility are unchanged.

**Tasks**

- [ ] Add the `auditor_access.otp_hmac` keyring configuration and validation.
- [ ] Add typed key IDs and secret-safe key material handling.
- [ ] Implement the versioned, grant-bound HMAC-SHA-256 message contract.
- [ ] Implement constant-time verification and sanitized error types.
- [ ] Separate and name the existing high-entropy session-token SHA-256 helper.
- [ ] Add configuration, redaction, fixed-vector, separation, and rejection
      tests.

**Notes**

- Runtime OTP persistence remains unchanged until ticket 002; see
  [Digest Contract](../spec.md#digest-contract).
