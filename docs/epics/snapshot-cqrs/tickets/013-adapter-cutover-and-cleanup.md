# 013 - Adapter Cutover and Cleanup

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 005, 011, 012 · **Spec:** [spec.md](../spec.md#migration-and-compatibility)

**Summary** - Finish typed-handler wiring across every adapter, remove legacy
service and raw-message boundaries, and close the epic with full verification.

**Acceptance criteria**

- [ ] Given any route, MCP tool, OAuth endpoint, or worker, when composed, then it receives concrete required handlers and cannot dynamically dispatch operations.
- [ ] Given the final source tree, when searched, then old service façades, raw outbox construction, and SQL-shaped write methods are absent.
- [ ] Given the pre-refactor external contracts, when the full suite runs, then behavior remains compatible.

**Tasks**

- [ ] Migrate every remaining adapter and composition-root binding.
- [ ] Remove service façades and obsolete repository methods.
- [ ] Remove raw JSON integration-message construction.
- [ ] Reconcile the spec and all ticket/README statuses.
- [ ] Run `.expect(` audit, `make check`, and code review against `56ae208`; fix findings and rerun checks.
