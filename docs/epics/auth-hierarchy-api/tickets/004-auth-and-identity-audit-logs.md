# 004 — Auth & Identity Audit Logs

**Status:** Todo · **Depends on:** 001, 002, paseto-token-migration/006, reliability-observability/005 · **Spec:** [API-token spec](../../paseto-token-migration/spec.md#audit-and-secret-handling)

**Summary** — Emit structured identity and access audit logs for login, workspace
creation, membership change, and user API-token issue/revoke.

**Acceptance criteria**

- [ ] Given an audited operation, when its log is emitted, then it can be
  attributed to a `user_id` and, when applicable, an `api_token_id`.
- [ ] Given an instrumented mutation, when it commits, then one audit log is
  emitted after commit; when it rolls back, then no success audit log is emitted.
- [ ] Given the instrumented operations, when each runs, then the full event set is emitted (`user.logged_in`, `workspace.created`, `workspace.member_added`/`_removed`, `api_token.issued`/`_revoked`).
- [ ] Given any emitted audit log, when its fields are inspected, then it
  contains no raw key/token/hash (credentials referenced by
  `api_token_id`).
- [ ] Given repeated authenticated requests from one user, when they are processed, then `user.logged_in` is deduplicated (not one row per request).
- [ ] Given an identity audit log, when it is emitted, then `type = "audit_log"`
  and the shared correlation fields are present.

**Tasks**

- [ ] Integrate the structured fields from `reliability-observability/005`.
- [ ] Emit workspace/member logs after successful 002 operations.
- [ ] Emit API-token lifecycle logs after successful
  `paseto-token-migration/002` operations plus deduped `user.logged_in` from 001
  middleware.
- [ ] Tests (attribution, no success log on rollback, no-secrets, dedup).

**Notes**

- Audit records are application logs, not Postgres rows or outbox messages.
- The sink, retention, field contract, and known post-commit crash window are
  documented in the Reliability and Observability spec.
- There is no Proofplane audit read/query API in the MVP.
- Revised on 2026-06-17 to replace actor/credential events with user-owned
  PASETO token events from the PASETO Token Migration spec.
- Revised on 2026-06-19 to depend on the compact opaque API-token pivot; event
  names and identifier-only audit fields remain unchanged.
