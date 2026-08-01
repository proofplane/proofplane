# 006 - Upload Operations And Guidance

**Status:** Done · **Depends on:** 005, reliability-observability/005, reliability-observability/007 · **Spec:** [spec.md](../spec.md#lifecycle-audit-and-metrics)

**Summary** - Make machine uploads safe to operate and easy for agents to use
through structured audit events, low-cardinality metrics, end-to-end failure
coverage, and updated MCP guidance.

**Acceptance criteria**

- [x] Given grant issuance and successful completion, when audit logs are
  captured, then attributable success events appear only after their
  transactions commit.
- [x] Given failures, retries, duplicate or losing attempts, when audit logs and
  metrics are inspected, then no false success or sensitive upload value is
  present.
- [x] Given representative traffic, when metrics are read, then issuance,
  validation, bytes, completion, replay, concurrency, dependency, and cleanup
  outcomes are visible with low-cardinality labels.
- [x] Given an agent reads the submitting-evidence guide, when choosing a flow,
  then human browser upload and trusted-runtime machine upload are clearly
  distinguished.
- [x] Given the full integration suite, when existing browser uploads run, then
  their behavior remains unchanged.

**Tasks**

- [x] Emit the machine-grant issuance and upload-completion audit events.
- [x] Instrument grant and transfer operations with bounded metric labels.
- [x] Add captured-log and metric tests for forbidden sensitive values.
- [x] Update MCP tool descriptions and `submitting-evidence` guidance.
- [x] Add an end-to-end prepare, stream, poll, scan, and finalize test.
- [x] Reconcile the spec and reliability cross-references with shipped
  behavior.
- [x] Run focused tests and finish with `make check`.

**Notes**

- The specs were revised to record the shipped audit events, bounded metric
  families, and repository-standard `proofplane_` prefix.
