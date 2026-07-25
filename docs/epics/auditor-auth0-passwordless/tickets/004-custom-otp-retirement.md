# 004 - Custom OTP Retirement

**Status:** Todo · **Depends on:** 003 · **Spec:**
[spec.md](../spec.md#migration-and-cutover)

**Summary** - After the hosted flow is active everywhere and old codes have
expired, remove the custom OTP implementation and its operational secrets so
Proofplane has only one auditor authentication path.

**Acceptance criteria**

- [ ] Given every API instance uses the Auth0 flow and at least ten minutes have
      elapsed since cutover, when cleanup ships, then the OTP table, repository,
      service, routes, mail adapter, and Resend configuration are absent.
- [ ] Given a client calls a removed JSON or browser OTP endpoint, when the
      request is handled, then it cannot authenticate or create a session and
      follows the documented removed-route behavior.
- [ ] Given an invitation issued before cleanup, when it remains active, then it
      can start the Auth0 flow without being reissued.
- [ ] Given a legacy auditor session, when it remains within its seven-day
      lifetime, then it continues to work and can still be revoked or logged
      out.
- [ ] Given the repository is searched after cleanup, when obsolete OTP and
      mail-delivery symbols are inspected, then no runtime path, secret, or
      misleading setup documentation remains.

**Tasks**

- [ ] Add the post-cutover migration that drops `auditor_access_otps`.
- [ ] Remove OTP request/verify routes, payloads, rendering, and error mapping.
- [ ] Remove OTP service and repository behavior while retaining session
      loading, creation, revocation, and grant checks.
- [ ] Remove the auditor mail adapter, direct Resend configuration, and
      associated production/local setup.
- [ ] Replace obsolete integration helpers and tests with Auth0-flow coverage.
- [ ] Update API fixtures, release notes, and operational documentation for the
      removed endpoints and rollback boundary.
- [ ] Run formatting, Clippy, unit, and Docker-backed integration checks.

**Notes**

- This ticket is a later deployment boundary, not part of the first cutover
  release; see [Migration And Cutover](../spec.md#migration-and-cutover).
