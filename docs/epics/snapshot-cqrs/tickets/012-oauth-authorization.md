# 012 - OAuth Authorization

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 007 · **Spec:** [spec.md](../spec.md#aggregate-boundaries)

**Summary** - Make an OAuth authorization flow the write aggregate for request,
subject, consent, code, cancellation, and consumption, with separate consent
and client lookup queries.

**Acceptance criteria**

- [ ] Given a valid authorization flow, when each command is handled, then state advances once and issued artifacts remain compatible.
- [ ] Given cancellation, replay, mismatch, or expiry, when handled, then no code or token is issued.
- [ ] Given existing OAuth clients, when cut over, then redirects, errors, tokens, and consent context remain unchanged.

**Tasks**

- [ ] Complete OAuth authorization aggregate behavior.
- [ ] Add complete-snapshot repository and lifecycle handlers.
- [ ] Add consent-context and client-facing query handlers.
- [ ] Migrate routes and authentication adapters.
- [ ] Add protocol compatibility, replay, expiry, and rollback tests.
