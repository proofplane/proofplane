# 002 — Workspace Self-Onboarding & Membership

**Status:** Done · **Depends on:** 001 · **Spec:** [spec.md](../spec.md#spicedb-schema-authzspicedbproofplanezed)

**Summary** — Let an authenticated human create a workspace they own and manage who can administer it. Human authorization is answered from Postgres (`workspace_memberships`), the transactional source of truth.

**Acceptance criteria**

- [x] Given an authenticated user, when they `POST /workspaces`, then the workspace is created and they are recorded as its `owner` in Postgres.
- [x] Given a request with no valid token, when it calls `POST /workspaces`, then it returns 401.
- [x] Given an authenticated user, when they `GET /workspaces`, then only their workspaces are returned, each with their role.
- [x] Given a caller who is not an owner/admin, when they add or remove a member, then it returns 404 (indistinguishable from an absent workspace).
- [x] Given a workspace with a single owner, when a request removes the last owner, then it is rejected.
- [x] Given existing data-plane routes and API-key auth, when this ships, then they behave exactly as before.
- ~~Given the deployed schema, when relationships are inspected, then `platform` exists yet no platform tuples are present.~~ **Descoped:** the dormant `platform` superadmin tier was dropped to avoid dead code.

**Tasks**

- [x] `workspace_memberships` migration (`V003`) + txn-scoped repo methods.
- [x] `POST`/`GET /workspaces` (auto-owner, atomic workspace+membership insert), new router gated by `authenticate_user`.
- [x] Member add/remove + last-owner guard; `manage_members` answered from Postgres role.
- [x] Tests + seed data.

**Notes**

- **Deviation from spec — human plane authorizes from Postgres, not SpiceDB.** The spec modelled `owner`/`admin` in SpiceDB with an outbox dual-write (best-effort sync write + worker reconciliation). We dropped that: human membership already lives in `workspace_memberships`, written transactionally, so `manage_*` reads role directly from Postgres — no SpiceDB projection, no sync write-through, no worker reconciliation, no read-your-writes gap. SpiceDB remains the engine for the **actor data plane** (`member` → evidence/controls) only. Revisit if human/workspace authorization ever grows more relational than owner/admin (team hierarchies, org→workspace inheritance, the platform tier).
- `POST /workspaces` is the account-level bootstrap (only management route not workspace-scoped); the workspace row and the owner membership row commit in one transaction.
- **`POST /workspaces/{id}/members` was later removed.** The by-`user_id` add it provided could not onboard anyone who had not already logged in, and no caller can obtain the internal `user_id` it required — an unusable flow. A realistic invite-by-email onboarding flow is the intended replacement but is not yet scoped; `DELETE .../members/{user_id}` (remove) is unchanged, and the membership-insert capability is retained for that future flow.
- Scope: the last-owner guard lives in the service/repo layer (covers the remove path) and is ready for a future role-change route. No `platform` superadmin tier (descoped above).
