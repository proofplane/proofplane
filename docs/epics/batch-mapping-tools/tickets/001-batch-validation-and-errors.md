# 001 — Batch Validation and Errors

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#semantics)

**Summary** — Add the shared batch-shape validation and typed error carrying the
offending IDs, so all eight batch tools reject malformed batches identically and
report every bad ID in one response instead of failing one at a time.

**Acceptance criteria**

- [x] Given a batch of 50 items, when it is validated, then it passes; given 51, then it is rejected as `batch_too_large` reporting the limit and the received count.
- [x] Given an empty item list, when it is validated, then it is rejected as `empty_batch` and no database work is attempted.
- [x] Given a list repeating the same counterpart ID, when it is validated, then it is rejected as `duplicate_ids` listing each repeated ID.
- [x] Given a rejected batch, when it is rendered for MCP, then every offending ID appears in the response, not just the first.
- [x] Given the existing single-pair mapping tools, when this ships, then their arguments, responses, and error shapes are unchanged.

**Tasks**

- [x] Add the batch validation helper (empty, size cap, duplicates) beside the existing domain validation.
- [x] Add the typed batch error variants carrying offending ID sets.
- [x] Map the batch error to its MCP error response with the ID list in the payload.
- [x] Tests for each rejection case and the at-cap boundary.

**Notes**

- Validation runs before any transaction opens — see the spec's Semantics
  section for why the cap is 50 and why duplicates are rejected rather than
  collapsed.
- Shipped as a dedicated `BatchError` in `src/domain/batch.rs` rather than new
  `DomainError` variants: the existing `FieldIssue` renderer flattens everything
  to `{field, message}` strings, and batch errors need a real JSON array of IDs.
- `validate_batch` takes a key extractor so one helper serves both bare ID lists
  and `{control_id, rationale}` objects.
- Unknown-ID reporting (`BatchError::Unknown`) is deferred to 002, which adds the
  query that detects it. This ticket covers only the checks that run before any
  database work.
