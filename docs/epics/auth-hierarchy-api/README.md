# Auth Hierarchy API Epic

Customer self-onboarding for the Proofplane API: a human signs up via Auth0,
self-onboards by creating a workspace they own, then creates actors and issues API
keys scoped to that workspace. Humans **manage**; actors **do work**; the two
planes never cross.

Full rationale, schema, authorization model, and decisions live in
[spec.md](./spec.md) — the single source of
technical depth. Tickets below are lean handoff units that link into it.

This epic is **API-only**; its frontend is the parallel `self-onboarding-ui` epic.

## Tickets

| Ticket                                                                                                   | Status | Notes                                                                                                                                                                                              |
| -------------------------------------------------------------------------------------------------------- | ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 001. [Auth0 User Identity & JIT Provisioning](./tickets/001-auth0-user-identity-and-jit-provisioning.md) | Done   | `users` table, `TokenVerifier` (jwtk/RS256+JWKS), `authenticate_user`, JIT provisioning, `GET /me`.                                                                                                |
| 002. [Workspace Self-Onboarding & Membership](./tickets/002-workspace-self-onboarding-and-membership.md) | Done   | `workspace_memberships`, `POST`/`GET /workspaces`, member add/remove, last-owner guard. Human `manage_*` answered from Postgres (SpiceDB was the actor data plane until 003 removed it); dormant `platform` tier descoped. |
| 003. [Actor & API Key Management](./tickets/003-actor-and-api-key-management.md)                         | Done   | Workspace-scoped actors (`workspace_id` NOT NULL), multi-credential rotation, `manage_actors`, create/list actors, issue/revoke keys. **SpiceDB removed**: data-plane authz is now Postgres workspace binding + per-actor permission grants. |
| 004. [Auth & Identity Audit Logs](./tickets/004-auth-and-identity-audit-logs.md)                         | Todo   | Emit structured user, workspace, membership, and API-token lifecycle application logs.                                                                                                               |

## Sequencing

- **001** is foundational: every management route consumes its `UserContext`.
- **002** depends on 001 and adds the human management plane (`owner`/`admin`
  membership in Postgres, authorized directly from `workspace_memberships`); land
  it before 003. The dormant platform-superadmin scaffolding from the spec was
  dropped to avoid dead code.
- **003** depends on 002. It binds actors to exactly one workspace
  (`actors.workspace_id` NOT NULL), supports multiple credentials per actor, and
  **removes SpiceDB entirely**: data-plane access is authorized from Postgres —
  the actor must belong to the path workspace and hold the specific permission
  grant for the route (the six read/write × evidence-requests / submissions /
  controls permissions ported into an `actor_permissions` table).
- **004** depends on 001-002 and `paseto-token-migration/002` for the operations
  it instruments, and on `reliability-observability/005` for the shared
  audit-log field contract.
