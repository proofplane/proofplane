# 010 - Policies

**Status:** Todo · **Triage:** ready-for-agent · **Depends on:** 008 · **Spec:** [spec.md](../spec.md#aggregate-boundaries)

**Summary** - Convert policy definition, archive lifecycle, and control mappings
into commands and keep document eligibility in orchestration.

**Acceptance criteria**

- [ ] Given valid policy commands, when handled, then definition, mappings, and archive lifecycle are saved as a complete snapshot.
- [ ] Given archived or unauthorized state, when mutation is attempted, then it is rejected without partial mappings.
- [ ] Given catalog/detail consumers, when queried, then existing filtering, ordering, and DTOs remain unchanged.

**Tasks**

- [ ] Complete policy aggregate lifecycle and mapping behavior.
- [ ] Add complete-snapshot repository and command handlers.
- [ ] Add catalog and detail query handlers.
- [ ] Migrate HTTP/MCP callers and retain eligibility orchestration.
- [ ] Add lifecycle, rollback, and query tests.
