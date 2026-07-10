# 004 — Auth & Identity Audit Logs

**Status:** Done · **Depends on:** 001, 002, API Token And PASETO Migration (done, archived), reliability-observability/005 · **Spec:** API Token And PASETO Migration spec (archived — folder removed in commit a36b836)

**Summary** — Emit structured identity and access audit logs for login, workspace
creation, membership change, and user API-token issue/revoke.

**Acceptance criteria**

- [x] Given an audited operation, when its log is emitted, then it can be
  attributed to a `user_id` and, when applicable, an `api_token_id`.
- [x] Given an instrumented mutation, when it commits, then one audit log is
  emitted after commit; when it rolls back, then no success audit log is emitted.
- [x] Given the instrumented operations, when each runs, then the full event set is emitted (`user.logged_in`, `workspace.created`, `workspace.member_added`/`_removed`, `api_token.issued`/`_revoked`).
- [x] Given any emitted audit log, when its fields are inspected, then it
  contains no raw key/token/hash (credentials referenced by
  `api_token_id`).
- [x] Given a user calls `POST /login`, when authentication succeeds, then
  `user.logged_in` is emitted and the user's `last_login_at` is updated every
  time; `GET /me` remains a profile read and does not emit login audit logs.
- [x] Given an identity audit log, when it is emitted, then `type = "audit_log"`
  and the shared correlation fields are present.

**Tasks**

- [x] Integrate the structured fields from `reliability-observability/005`.
- [x] Emit workspace/member logs after successful 002 operations.
- [x] Emit API-token lifecycle logs after successful API-token operations (from
  the archived API Token And PASETO Migration epic) plus explicit
  `user.logged_in` from `POST /login`.
- [x] Tests (attribution, no success log on rollback, no-secrets, explicit login).

**Notes**

- Audit records are application logs, not Postgres rows or outbox messages.
- The field contract and known post-commit crash window are documented in the
  Reliability and Observability spec. Production routing and retention are
  deferred to deployment planning.
- There is no Proofplane audit read/query API in the MVP.
- Revised on 2026-06-17 to replace actor/credential events with user-owned
  PASETO token events from the PASETO Token Migration spec.
- Revised on 2026-06-19 to depend on the compact opaque API-token pivot; event
  names and identifier-only audit fields remain unchanged.
- Revised on 2026-06-22 to make `POST /login` the explicit login event, update
  `users.last_login_at` on every successful login, and keep `GET /me` read-only.
- 2026-07-09: Historical. PR #42 removed `ppat_` API tokens, so the
  `api_token.issued`/`_revoked` events and the `api_token_id` audit field no
  longer apply; MCP OAuth agent connections are the current non-human actor.
  The API Token And PASETO Migration epic this ticket instrumented was archived
  (folder removed in commit a36b836), so its links here are plain references
  rather than navigable paths.
