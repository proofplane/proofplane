# 005 - Auditor Access Workflows

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#aggregate-boundaries)

**Summary** - Move auditor grants, sessions, authentication transactions, and
authentication completion to explicit commands while keeping locator and
portal views as queries.

**Acceptance criteria**

- [ ] Given valid auditor authority, when each lifecycle command runs, then only its owning aggregate changes.
- [ ] Given an invalid digest, expiry, replay, or cross-workspace request, when handled, then access is denied without partial state.
- [ ] Given existing portal users, when cut over, then login and portal behavior are unchanged.

**Tasks**

- [ ] Complete aggregate lifecycle behavior and repositories.
- [ ] Add grant, session, transaction, and completion handlers.
- [ ] Add digest locator and portal query handlers.
- [ ] Migrate authentication and route callers.
- [ ] Add success, rejection, replay, and rollback tests.
