# 007 - Evidence Lifecycle Audit Logs

**Status:** Done · **Depends on:** 005, Evidence Lifecycle Completion (done, archived) · **Spec:**
[spec.md](../spec.md#evidence-lifecycle-audit-events)

**Summary** - Emit structured audit logs for evidence submission creation,
submission acceptance, download grant issuance and redemption, scan terminal
outcomes, and finalization so lifecycle-sensitive evidence actions are
attributable without exposing secrets.

**Acceptance criteria**

- [ ] Given successful evidence lifecycle operations, when logs are captured,
  then one attributable `type = "audit_log"` record is emitted for each audited
  lifecycle transition.
- [ ] Given rejected requests, rolled-back mutations, retryable worker failures,
  duplicate delivery, or stale delivery, when logs are captured, then no false
  success audit record is emitted.
- [ ] Given audit fields are inspected, then workspace, user, agent connection,
  system client, request correlation, event name, outcome, grant ID, submission
  ID, and submission ID may appear where applicable. (Actor is the agent
  connection ID, not an API token ID — `ppat_` removed in PR #42.)
- [ ] Given sensitive values exist during the flow, then raw grant tokens, API
  tokens, authorization headers, submission bytes, object keys treated as
  storage internals, submission summaries and descriptions, scanner raw error
  strings, and credentials are absent.

**Tasks**

- [ ] Define stable evidence audit event names and allowed fields in the
  Reliability spec.
- [ ] Instrument submission creation and submission acceptance after successful
  commits.
- [ ] Instrument download grant issuance and redemption without logging the
  PASETO token.
- [ ] Instrument scan and finalization outcomes, with duplicate/stale deliveries
  omitted or explicitly non-success.
- [ ] Add captured-log tests for success, rollback/no-false-success,
  retry/no-false-success, duplicate/stale delivery, and sensitive-context
  exclusion.
- [ ] Update the evidence lifecycle epic pointer so audit logging ownership is
  clear.
