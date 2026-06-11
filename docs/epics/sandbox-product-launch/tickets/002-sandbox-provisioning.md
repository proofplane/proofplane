# 002 - Sandbox Provisioning

**Status:** Todo · **Depends on:** auth-hierarchy-api/003 · **Spec:** [spec.md](../spec.md#account-and-sandbox-model)

**Summary** - Add idempotent create-or-resume sandbox provisioning with
workspace-scoped starter records and an AI-agent actor.

**Acceptance criteria**

- [ ] Given a user without a sandbox, when provisioning runs, then one sandbox
  workspace, owner membership, starter controls, mapped request, and agent actor
  are created.
- [ ] Given the same user retries or returns, when provisioning runs, then the
  existing sandbox is resumed without duplicate confusing workspaces.
- [ ] Given concurrent provisioning or partial failure, when retried, then the
  workflow converges to one complete sandbox.
- [ ] Given another user or standard workspace, when sandbox data is requested,
  then tenant and mode isolation are enforced server-side.

**Tasks**

- [ ] Add workspace mode and owner-to-sandbox lookup migration.
- [ ] Add transactional/idempotent provisioning service.
- [ ] Add create-or-resume management endpoint.
- [ ] Add realistic starter fixture builders.
- [ ] Add concurrency, retry, and tenant-isolation integration tests.
