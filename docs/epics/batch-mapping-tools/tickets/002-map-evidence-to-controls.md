# 002 — Map Evidence to Controls

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#implementation-shape)

**Summary** — Add the `map_evidence_to_controls` MCP tool: one evidence anchor
fanned out to many controls, each with its own rationale, in a single
transaction. This is the reference implementation the other batch tools follow.

**Acceptance criteria**

- [x] Given an evidence ID and several `{control_id, rationale}` items, when the tool is called, then every mapping is created and the response returns the count and the mapped control IDs.
- [x] Given a batch containing a control ID that does not exist in the workspace, when the tool is called, then the call fails naming every unknown control ID and no mapping from that batch is created.
- [x] Given a batch error carrying several unknown IDs, when it is rendered for MCP, then every offending ID appears in the response, not just the first.
- [x] Given a batch containing a pair that is already mapped, when the tool is called, then the call fails and no mapping from that batch is created.
- [x] Given an evidence ID from another workspace, when the tool is called, then it is rejected as not found.
- [x] Given a connection without `WriteControls`, when the tool is called, then it is rejected and nothing is written.
- [x] Given a successful batch, when it completes, then exactly one `evidence_control_mappings.created` audit event is emitted carrying the evidence ID, the full control ID list, and the item count.

**Tasks**

- [x] Add the batch insert repository method: anchor check, per-item resolve loop, per-item insert loop.
- [x] Add the `BatchError::Unknown { field, ids }` variant and its MCP rendering arm (deferred here from 001, which had no producer for it).
- [x] Detect the unknown-ID set with a pre-insert per-item resolve loop (not a short `RETURNING` count — see Notes).
- [x] Add the service method wrapping it in the agent-connection workspace transaction.
- [x] Add the `#[tool]` and register it on the controls tool router.
- [x] Emit the batch audit event.
- [x] Integration tests covering success, unknown IDs, duplicate pair, oversize, empty, cross-workspace, already-mapped rollback, and authorization.

**Notes**

- Items are `{control_id, rationale}` objects rather than bare IDs because
  rationale is per-pair — see the spec's Implementation shape section.
- The existing single-pair `map_evidence_to_control` stays as-is.
- Detection uses a per-item resolve loop *before* any insert, not the spec's
  original `UNNEST` insert + short-`RETURNING`-count + re-query. Postgres aborts
  the whole transaction on the conflicting insert, so the re-query could not run
  — see the spec revision note in the Implementation shape section.
- The success response is lean: `{ evidence_id, count, control_ids }`. Callers
  that want the full mapping objects read `list_evidence_control_mappings`.
