# 004 — Auth & Identity Audit Logs

**Status:** Todo · **Depends on:** 001, 002, 003, reliability-observability/005 · **Spec:** [spec.md](../spec.md#build-order)

**Summary** — Emit structured identity and access audit logs for login, workspace
creation, membership change, actor creation, and key issue/revoke.

**Acceptance criteria**

- [ ] Given an audited operation, when its log is emitted, then it can be
  attributed to a `user_id`, an `actor_id`, or both.
- [ ] Given an instrumented mutation, when it commits, then one audit log is
  emitted after commit; when it rolls back, then no success audit log is emitted.
- [ ] Given the instrumented operations, when each runs, then the full event set is emitted (`user.logged_in`, `workspace.created`, `workspace.member_added`/`_removed`, `actor.created`, `api_credential.issued`/`_revoked`).
- [ ] Given any emitted audit log, when its fields are inspected, then it
  contains no raw key/token/hash (credentials referenced by
  `credential_id`/`key_id`).
- [ ] Given repeated authenticated requests from one user, when they are processed, then `user.logged_in` is deduplicated (not one row per request).
- [ ] Given an identity audit log, when it is emitted, then `type = "audit_log"`
  and the shared correlation fields are present.

**Tasks**

- [ ] Integrate the structured fields from `reliability-observability/005`.
- [ ] Emit workspace/member logs after successful 002 operations.
- [ ] Emit actor/credential logs after successful 003 operations plus deduped
  `user.logged_in` from 001 middleware.
- [ ] Tests (attribution, no success log on rollback, no-secrets, dedup).

**Notes**

- Audit records are application logs, not Postgres rows or outbox messages.
- The sink, retention, field contract, and known post-commit crash window are
  documented in the Reliability and Observability spec.
- There is no Proofplane audit read/query API in the MVP.
