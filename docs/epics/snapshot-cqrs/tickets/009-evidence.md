# 009 - Evidence

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 008 · **Spec:** [spec.md](../spec.md#aggregate-boundaries)

**Summary** - Convert evidence definition, status, and control mappings into
aggregate commands and introduce dedicated evidence read models.

**Acceptance criteria**

- [ ] Given valid evidence commands, when handled, then definition, status, and mappings obey aggregate invariants.
- [ ] Given invalid state or cross-workspace mappings, when handled, then no snapshot is partially saved.
- [ ] Given list/detail/mapping consumers, when queried, then existing filtering, ordering, and DTOs are preserved.

**Tasks**

- [ ] Complete evidence aggregate lifecycle and mapping behavior.
- [ ] Add complete-snapshot repository and command handlers.
- [ ] Add list, detail, and reverse-mapping queries.
- [ ] Migrate HTTP/MCP callers.
- [ ] Add lifecycle, mapping, rollback, and query tests.
