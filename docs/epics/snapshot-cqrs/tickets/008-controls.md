# 008 - Controls

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 003 · **Spec:** [spec.md](../spec.md#aggregate-boundaries)

**Summary** - Move control definition and framework-reference mutations behind
the control aggregate while retaining framework catalogs as read-only data.

**Acceptance criteria**

- [ ] Given valid control input and framework references, when create or replace is handled, then the complete snapshot is saved atomically.
- [ ] Given invalid, missing, or cross-workspace references, when handled, then no partial mapping is saved.
- [ ] Given existing catalog consumers, when cut over, then ordering and DTOs are unchanged.

**Tasks**

- [ ] Complete control aggregate mapping behavior.
- [ ] Add complete-snapshot repository.
- [ ] Add create and replace command handlers.
- [ ] Add framework/control catalog query handlers and migrate callers.
- [ ] Add validation, rollback, and read-model tests.
