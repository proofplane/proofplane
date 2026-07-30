# 006 - Upload Operations And Guidance

**Status:** Todo · **Depends on:** 005, reliability-observability/005, reliability-observability/007 · **Spec:** [spec.md](../spec.md#lifecycle-audit-and-metrics)

**Summary** - Make machine uploads safe to operate and easy for agents to use
through structured audit events, low-cardinality metrics, end-to-end failure
coverage, and updated MCP guidance.

**Acceptance criteria**

- [ ] Given grant issuance and successful completion, when audit logs are
  captured, then attributable success events appear only after their
  transactions commit.
- [ ] Given failures, retries, duplicate or losing attempts, when audit logs and
  metrics are inspected, then no false success or sensitive upload value is
  present.
- [ ] Given representative traffic, when metrics are read, then issuance,
  validation, bytes, completion, replay, concurrency, dependency, and cleanup
  outcomes are visible with low-cardinality labels.
- [ ] Given an agent reads the submitting-evidence guide, when choosing a flow,
  then human browser upload and trusted-runtime machine upload are clearly
  distinguished.
- [ ] Given the full integration suite, when existing browser uploads run, then
  their behavior remains unchanged.

**Tasks**

- [ ] Emit the machine-grant issuance and upload-completion audit events.
- [ ] Instrument grant and transfer operations with bounded metric labels.
- [ ] Add captured-log and metric tests for forbidden sensitive values.
- [ ] Update MCP tool descriptions and `submitting-evidence` guidance.
- [ ] Add an end-to-end prepare, stream, poll, scan, and finalize test.
- [ ] Reconcile the spec and reliability cross-references with shipped
  behavior.
- [ ] Run focused tests and finish with `make check`.
