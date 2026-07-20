# 002 — Map Evidence to Controls

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#implementation-shape)

**Summary** — Add the `map_evidence_to_controls` MCP tool: one evidence anchor
fanned out to many controls, each with its own rationale, in a single
transaction. This is the reference implementation the other batch tools follow.

**Acceptance criteria**

- [ ] Given an evidence ID and several `{control_id, rationale}` items, when the tool is called, then every mapping is created and all of them are returned with their rationales.
- [ ] Given a batch containing a control ID that does not exist in the workspace, when the tool is called, then the call fails naming every unknown control ID and no mapping from that batch is created.
- [ ] Given a batch error carrying several unknown IDs, when it is rendered for MCP, then every offending ID appears in the response, not just the first.
- [ ] Given a batch containing a pair that is already mapped, when the tool is called, then the call fails and no mapping from that batch is created.
- [ ] Given an evidence ID from another workspace, when the tool is called, then it is rejected as not found.
- [ ] Given a connection without `WriteControls`, when the tool is called, then it is rejected and nothing is written.
- [ ] Given a successful batch, when it completes, then exactly one `evidence_control_mappings.created` audit event is emitted carrying the evidence ID, the full control ID list, and the item count.

**Tasks**

- [ ] Add the batch insert repository method using `UNNEST`, returning the created rows.
- [ ] Add the `BatchError::Unknown { field, ids }` variant and its MCP rendering arm (deferred here from 001, which had no producer for it).
- [ ] Compute the unknown-ID set from a short `RETURNING` count.
- [ ] Add the service method wrapping it in the agent-connection workspace transaction.
- [ ] Add the `#[tool]` and register it on the controls tool router.
- [ ] Emit the batch audit event.
- [ ] Integration tests covering success, unknown IDs, duplicate pair, cross-workspace, and rollback.

**Notes**

- Items are `{control_id, rationale}` objects rather than bare IDs because
  rationale is per-pair — see the spec's Implementation shape section.
- The existing single-pair `map_evidence_to_control` stays as-is.
