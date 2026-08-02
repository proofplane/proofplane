# 006 - Users and Workspaces

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#aggregate-boundaries)

**Summary** - Convert identity provisioning/login and workspace membership
mutations to handlers, with membership and last-owner invariants owned by the
workspace aggregate.

**Acceptance criteria**

- [ ] Given a login or provisioning request, when handled, then the user snapshot records the correct lifecycle transition idempotently.
- [ ] Given membership changes, when handled, then the workspace aggregate prevents removal of its last owner.
- [ ] Given an unauthorized or cross-workspace command, when handled, then no user or workspace state changes.

**Tasks**

- [ ] Complete user and workspace aggregate behavior.
- [ ] Add complete-snapshot repositories and membership persistence.
- [ ] Add provisioning, login, workspace, and membership handlers.
- [ ] Migrate route and authentication callers.
- [ ] Add invariant, authorization, concurrency, and replay tests.
