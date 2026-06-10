# 002 — Workspace Self-Onboarding & Membership

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#spicedb-schema-authzspicedbproofplanezed)

**Summary** — Let an authenticated human create a workspace they own and manage who can administer it. Establishes the SpiceDB human-management model the rest of the epic builds on.

**Acceptance criteria**

- [x] Given an authenticated user, when they `POST /workspaces`, then the workspace is created and they are its `owner` in Postgres and SpiceDB.
- [x] Given a request with no valid token, when it calls `POST /workspaces`, then it returns 401.
- [x] Given an authenticated user, when they `GET /workspaces`, then only their workspaces are returned, each with their role.
- [x] Given a caller without `manage_members`, when they add or remove a member, then it returns 404.
- [x] Given a workspace with a single owner, when a request removes the last owner, then it is rejected.
- [x] Given a membership change whose synchronous SpiceDB write fails, when the worker runs, then it reconciles from the outbox so both stores converge.
- [x] Given existing data-plane routes and API-key auth, when this ships, then they behave exactly as before.
- ~~Given the deployed schema, when relationships are inspected, then `platform` exists yet no platform tuples are present.~~ **Descoped:** the dormant `platform` superadmin tier was dropped to avoid dead code; add it when a platform tier is actually needed.

**Tasks**

- [x] `workspace_memberships` migration (`V003`) + txn-scoped repo methods.
- [x] Extend `.zed` schema + `authorization::spicedb` adapter (user-subject write/delete + `manage_*` checks) + `WorkspaceAuthorizer`.
- [x] `POST`/`GET /workspaces` (auto-owner + dual-write), new router gated by `authenticate_user`.
- [x] Member add/remove + last-owner guard.
- [x] Worker reconciliation handler (worker gains a SpiceDB client) + dispatch arm.
- [x] Tests (incl. outbox reconciliation) + seed data.

**Notes**

- Dual-write = row + outbox in one txn → best-effort sync SpiceDB write → worker backstop. Detail in spec.
- `POST /workspaces` is the account-level bootstrap (only management route not workspace-scoped).
- Adding a member requires the target user to have logged in already — no invite flow.
- Scope: only `POST`/`DELETE` member endpoints shipped (per the spec's endpoint list); the last-owner guard lives in the service/repo layer (covers the remove path) and is ready for a future role-change route. No `platform` superadmin tier (descoped above).
