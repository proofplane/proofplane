# 002 — Workspace Self-Onboarding & Membership

**Status:** Todo · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#spicedb-schema-authzspicedbproofplanezed)

**Summary** — Let an authenticated human create a workspace they own and manage who can administer it. Establishes the SpiceDB human-management model the rest of the epic builds on.

**Acceptance criteria**

- [ ] Given an authenticated user, when they `POST /workspaces`, then the workspace is created and they are its `owner` in Postgres and SpiceDB.
- [ ] Given a request with no valid token, when it calls `POST /workspaces`, then it returns 401.
- [ ] Given an authenticated user, when they `GET /workspaces`, then only their workspaces are returned, each with their role.
- [ ] Given a caller without `manage_members`, when they add or remove a member, then it returns 404.
- [ ] Given a workspace with a single owner, when a request removes or demotes that last owner, then it is rejected.
- [ ] Given a membership change whose synchronous SpiceDB write fails, when the worker runs, then it reconciles from the outbox so both stores converge.
- [ ] Given the deployed schema, when relationships are inspected, then `platform` exists yet no platform tuples are present.
- [ ] Given existing data-plane routes and API-key auth, when this ships, then they behave exactly as before.

**Tasks**

- [ ] `workspace_memberships` migration + repo methods.
- [ ] Extend `.zed` schema + `authorization::spicedb` adapter + `WorkspaceAuthorizer` checks; deploy.
- [ ] `POST`/`GET /workspaces` (auto-owner + dual-write), new router gated by `authenticate_user`.
- [ ] Member add/remove + last-owner guard.
- [ ] Tests (incl. outbox reconciliation) + seed data.

**Notes**

- Dual-write = row + outbox in one txn → best-effort sync SpiceDB write → worker backstop. Detail in spec.
- `POST /workspaces` is the account-level bootstrap (only management route not workspace-scoped).
- Adding a member requires the target user to have logged in already — no invite flow.
