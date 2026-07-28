# 004 - Custom OTP Retirement

**Status:** Done · **Depends on:** 003 · **Spec:**
[spec.md](../spec.md#migration-and-cutover)

**Summary** - Remove the custom OTP implementation and its operational secrets
in the same clean cutover because the local database is resettable and no
auditor sessions need compatibility.

**Acceptance criteria**

- [x] Given the reset baseline is migrated, when its schema is inspected, then
      sessions require a nonblank Auth0 subject and no OTP table exists.
- [x] Given a client calls a removed JSON or browser OTP endpoint, when the
      request is handled, then it cannot authenticate or create a session and
      returns the normal not-found response.
- [x] Given a grant created after the local reset, when its invitation starts,
      then it enters the Auth0 flow without any OTP compatibility state.
- [x] Given the repository is searched after cleanup, when obsolete OTP and
      mail-delivery symbols are inspected, then no runtime path, secret, or
      misleading setup documentation remains.

**Tasks**

- [x] Remove `auditor_access_otps` from the resettable baseline schema.
- [x] Remove OTP request/verify routes, payloads, rendering, and error mapping.
- [x] Remove OTP service and repository behavior while retaining session
      loading, creation, revocation, and grant checks.
- [x] Remove the auditor mail adapter, direct Resend configuration, and
      associated production/local setup.
- [x] Replace obsolete integration helpers and tests with Auth0-flow coverage.
- [x] Update the epic spec, ticket index, and local configuration for the
      removed endpoints and reset boundary.
- [x] Run formatting, Clippy, unit, and Docker-backed integration checks.

**Notes**

- This cleanup is intentionally folded into the cutover. Reset the local
  database before running the revised baseline migrations.
