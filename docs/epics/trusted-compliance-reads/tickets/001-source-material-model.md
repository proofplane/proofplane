# 001 - Source Material Model

**Status:** Todo · **Depends on:** evidence-lifecycle-completion/001 · **Spec:** [spec.md](../spec.md#source-material-model)

**Summary** - Add durable curated compliance material and provenance mappings
without adding submission approval state.

**Acceptance criteria**

- [ ] Given valid material linked to a workspace control or Evidence Request,
  when created, then author, rationale, status, and provenance links persist.
- [ ] Given cross-workspace or inconsistent submission links, when written,
  then the transaction is rejected without partial mappings.
- [ ] Given existing submissions, controls, and mappings, when this ships, then
  their schemas and behavior remain unchanged.

**Tasks**

- [ ] Add source-material and mapping migrations.
- [ ] Add domain IDs, status parsing, and validation.
- [ ] Add transactional create/update/get/search repository operations.
- [ ] Add repository integration tests for invariants and rollback.
