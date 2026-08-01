# 004 - Policy Upload Operations

**Status:** Todo · **Depends on:** 003, reliability-observability/005, reliability-observability/007 · **Spec:** [spec.md](../spec.md#audit-and-metrics)

**Summary** - Make agent-native policy uploads safe to operate through
structured audit events, bounded metrics, full failure coverage, and regression
tests for the existing human workflow.

**Acceptance criteria**

- [ ] Given grant issuance and first successful completion, when audit logs are
  captured, then attributable success events appear only after commit.
- [ ] Given rejection, rollback, replay, or a losing race, when telemetry is
  inspected, then it contains no false success or sensitive transfer value.
- [ ] Given representative traffic, when metrics are read, then grant,
  validation, bytes, completion, replay, conflict, dependency, and cleanup
  outcomes are visible with low-cardinality labels.
- [ ] Given an agent follows the policies guide, when it has a trusted runtime,
  then it can prepare, transfer, and poll without confusing the human grant.
- [ ] Given the full integration suite, when browser policy document management
  runs, then upload, download, archive, and replacement behavior is unchanged.

**Tasks**

- [ ] Emit machine policy grant issuance and upload completion audit events.
- [ ] Instrument grant and transfer outcomes with bounded metric labels.
- [ ] Add captured-log and metric tests for forbidden sensitive values.
- [ ] Add end-to-end prepare, stream, poll, scan, and finalize coverage.
- [ ] Cover cleanup failure and machine-versus-browser concurrency.
- [ ] Reconcile the spec and policy guidance with shipped behavior.
- [ ] Run focused tests and finish with `make check`.

**Notes**

- Reuse the repository-wide cleanup metric; keep policy grant and attempt
  metrics distinct from evidence metrics for operational clarity.
