# 001 - Policy Domain And Persistence

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#persistence-model)

**Summary** - Add the workspace-scoped policy lifecycle and many-to-many
control relationships that every MCP, document, and auditor surface builds
on.

**Acceptance criteria**

- [x] Given valid metadata and zero or more workspace control IDs, when a
  policy is created, then it and all mappings commit atomically and can be read
  in deterministic order.
- [x] Given a duplicate active name or any duplicate, missing, or
  cross-workspace control reference, when creation or mapping is attempted,
  then the operation is rejected without partial state or existence leakage.
- [x] Given an active policy, when its name and description are updated or
  mappings are attached and detached, then unrelated mappings and document
  state remain unchanged.
- [x] Given a terminal or missing document, when the policy is archived, then
  normal reads hide it while retaining its rows; an in-progress document
  blocks archival.
- [x] Given existing control and evidence mappings, when policy persistence
  ships, then their schemas and behavior are unchanged.

**Tasks**

- [x] Add policy, mapping, ID, payload, validation, and error domain types.
- [x] Add migrations for policies, policy-control mappings, and the policy
  document schema prerequisite, including uniqueness and query indexes.
- [x] Add workspace-scoped repository reads and transactional create, update,
  archive, attach, and detach operations.
- [x] Add policy service orchestration and conflict/reference classification.
- [x] Add unit and Docker-backed integration tests for validation, ordering,
  rollback, tenancy, uniqueness, mapping mutation, and archival guards.

**Notes**

- Policy document persistence lands here so archival can enforce the
  in-progress guard; ticket 003 retains document lifecycle and worker work.
