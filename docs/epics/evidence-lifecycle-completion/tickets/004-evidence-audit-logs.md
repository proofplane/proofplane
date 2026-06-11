# 004 - Evidence Audit Logs

**Status:** Todo · **Depends on:** reliability-observability/005 · **Spec:** [spec.md](../spec.md#audit-logging)

**Summary** - Emit structured audit logs for submission creation, attachment
acceptance, download, scan outcomes, and finalization.

**Acceptance criteria**

- [ ] Given a successful submission, upload acceptance, download, scan terminal
  outcome, or finalization, when it completes, then one attributable
  `type = "audit_log"` record is emitted.
- [ ] Given a rejected request, rolled-back mutation, or retryable worker
  failure, when logs are captured, then no false success audit log is emitted.
- [ ] Given attachment activity, when audit fields are inspected, then metadata
  identifiers and outcomes may appear but file bytes, credentials, and scanner
  error strings do not.
- [ ] Given duplicate or stale worker delivery, when handled, then it is either
  omitted or logged with an explicit non-success outcome, never as a new
  successful lifecycle transition.

**Tasks**

- [ ] Define stable evidence audit event names and allowed fields.
- [ ] Instrument submission/upload/download service outcomes.
- [ ] Instrument scan and finalization handler outcomes.
- [ ] Add captured-log tests for success, rollback, retry, and secret exclusion.
