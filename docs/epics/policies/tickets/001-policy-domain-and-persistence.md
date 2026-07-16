# 001 - Policy Domain And Persistence

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#persistence-model)

**Summary** - Add the workspace-scoped policy lifecycle and many-to-many
control relationships that every MCP, attachment, and auditor surface builds
on.

**Acceptance criteria**

- [ ] Given valid metadata and zero or more workspace control IDs, when a
  policy is created, then it and all mappings commit atomically and can be read
  in deterministic order.
- [ ] Given a duplicate active name or any duplicate, missing, or
  cross-workspace control reference, when creation or mapping is attempted,
  then the operation is rejected without partial state or existence leakage.
- [ ] Given an active policy, when its name and description are updated or
  mappings are attached and detached, then unrelated mappings and attachment
  state remain unchanged.
- [ ] Given a terminal or missing attachment, when the policy is archived, then
  normal reads hide it while retaining its rows; an in-progress attachment
  blocks archival.
- [ ] Given existing control and evidence mappings, when policy persistence
  ships, then their schemas and behavior are unchanged.

**Tasks**

- [ ] Add policy, mapping, ID, payload, validation, and error domain types.
- [ ] Add migrations for policies and policy-control mappings, including active
  name uniqueness and workspace/query indexes.
- [ ] Add workspace-scoped repository reads and transactional create, update,
  archive, attach, and detach operations.
- [ ] Add policy service orchestration and conflict/reference classification.
- [ ] Add unit and Docker-backed integration tests for validation, ordering,
  rollback, tenancy, uniqueness, mapping mutation, and archival guards.
