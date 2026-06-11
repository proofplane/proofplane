# 005 - Structured Audit Logging

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#structured-audit-logs)

**Summary** - Establish the shared structured audit-log fields, remove the
dormant database table, and document the dedicated Cloud Logging sink contract.

**Acceptance criteria**

- [ ] Given an audit log, when serialized, then `type = "audit_log"` and the
  stable identity, correlation, operation, outcome, and object fields are
  present where applicable.
- [ ] Given credentials, object bytes, source-material bodies, or unbounded
  errors, when an audit log is emitted, then those values are absent.
- [ ] Given a rolled-back mutation, when logs are captured, then no success audit
  log is emitted; given a commit, then the success log is emitted afterward.
- [ ] Given the migration is applied, when the schema is inspected, then the
  unused `audit_events` table is absent and existing product data is unchanged.

**Tasks**

- [ ] Add shared audit-log field/event helpers over `tracing`.
- [ ] Add capture tests for required fields and prohibited values.
- [ ] Add the migration dropping `audit_events`.
- [ ] Document Cloud Logging sink filter, retention, IAM, and analysis access.
- [ ] Document the accepted post-commit logging crash window.
