# 004 — Auth & Identity Audit Events

**Status:** Todo · **Depends on:** 001, 002, 003 · **Spec:** [spec.md](../spec.md#build-order)

**Summary** — Record a durable audit trail for identity/access operations by populating the unused `audit_events` table: login, workspace creation, membership change, actor creation, key issue/revoke.

**Acceptance criteria**

- [ ] Given an audited operation, when its event is written, then it can be attributed to a `user_id`, an `actor_id`, or both.
- [ ] Given an instrumented operation, when it commits, then exactly one event is written in the same transaction; when it rolls back, then no event remains.
- [ ] Given the instrumented operations, when each runs, then the full event set is emitted (`user.logged_in`, `workspace.created`, `workspace.member_added`/`_removed`, `actor.created`, `api_credential.issued`/`_revoked`).
- [ ] Given any emitted event, when its payload is inspected, then it contains no raw key/token/hash (credentials referenced by `credential_id`/`key_id`).
- [ ] Given repeated authenticated requests from one user, when they are processed, then `user.logged_in` is deduplicated (not one row per request).
- [ ] Given an identity event, when it is written, then it is a local in-transaction Postgres row, not an outbox message.

**Tasks**

- [ ] Migration: `audit_events.user_id` + indexes.
- [ ] `AuditEventWriter` trait + Postgres impl (insert on caller's transaction); management-plane transactional helper.
- [ ] Emit workspace/member events from 002 ops.
- [ ] Emit actor/credential events from 003 ops + deduped `user.logged_in` from 001 middleware.
- [ ] Tests (attribution, rollback, no-secrets, dedup).

**Notes**

- Table currently references `actor_id` only; management events need `user_id`. Don't use the outbox — these are local in-txn rows.
- This is write-only: no audit read/query API, retention/export, or data-plane (evidence/controls) audit here.
