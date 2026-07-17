# 005 - Structured Audit Logging

**Status:** Done · **Depends on:** none · **Spec:** [spec.md](../spec.md#structured-audit-logs)

**Summary** - Establish a transport-neutral structured audit-event API over
`tracing` and verify the dormant database table is absent.

**Acceptance criteria**

- [x] Given an audit log, when serialized, then `type = "audit_log"` and the
  stable identity, correlation, operation, outcome, and object fields are
  present where applicable.
- [x] Given credentials, bearer grant tokens or URLs, internal object keys,
  submission or packet bytes, submission summaries or descriptions, or
  unbounded errors, when an audit log is emitted, then those values are absent.
- [x] Given a mutation caller, when the emission contract is followed, then a
  success event is emitted only after its service transaction returns
  successfully, with the acknowledged commit-to-log crash window documented.
- [x] Given the consolidated initial migration is applied to an empty database,
  when the schema is inspected, then the unused `audit_events` table is absent.

**Tasks**

- [x] Add shared audit-log field/event helpers over `tracing`.
- [x] Add capture tests for required fields and prohibited values.
- [x] Confirm the consolidated `V001` omits `audit_events` and no runtime code
  references it.
- [x] Defer routing, retention, IAM, and analysis infrastructure to future
  production-deployment planning.
- [x] Document the accepted post-commit logging crash window.

**Notes**

- Domain event emission remains in `auth-hierarchy-api/004` and
  `reliability-observability/007`.
- The 2026-06-22 spec revision records the infrastructure scope deferral.
