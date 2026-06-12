# 002 - Source Material API

**Status:** Todo · **Depends on:** 001, reliability-observability/005 · **Spec:** [spec.md](../spec.md#api-contract)

**Summary** - Expose authorized create, replace, retrieve, and filtered search
operations for curated source material.

**Acceptance criteria**

- [ ] Given valid authorized input, when material is created or replaced, then
  the response includes its provenance links and a structured audit log follows
  the successful commit.
- [ ] Given invalid links, empty content, or an unauthorized workspace, when a
  write is attempted, then it is rejected with no row or success audit log.
- [ ] Given a text/control/request query, when search runs, then only matching
  workspace material is returned with stable pagination.
- [ ] Given existing control and evidence APIs, when this ships, then their
  authorization and response contracts are unchanged.

**Tasks**

- [ ] Add source-material permissions to the authorization schema.
- [ ] Add service operations and DTO validation.
- [ ] Add REST routes and stable error mapping.
- [ ] Emit create/update/get/search structured audit logs.
- [ ] Add API integration tests for writes, filters, and tenant isolation.
