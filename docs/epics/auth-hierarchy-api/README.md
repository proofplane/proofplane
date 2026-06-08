# Auth Hierarchy API Epic

Customer self-onboarding for the Proofplane API: a human signs up via Auth0,
self-onboards by creating a workspace they own, then creates actors and issues API
keys scoped to that workspace. Humans **manage**; actors **do work**; the two
planes never cross.

Full rationale, schema, SpiceDB model, and decisions live in
[spec.md](./spec.md) — the single source of
technical depth. Tickets below are lean handoff units that link into it.

This epic is **API-only**; its frontend is the parallel `self-onboarding-ui` epic.

## Tickets

| Ticket                                                                                                   | Status | Notes                                                                                                                                    |
| -------------------------------------------------------------------------------------------------------- | ------ | ---------------------------------------------------------------------------------------------------------------------------------------- |
| 001. [Auth0 User Identity & JIT Provisioning](./tickets/001-auth0-user-identity-and-jit-provisioning.md) | Todo   | `users` table, `TokenVerifier` (jwtk/RS256+JWKS), `authenticate_user`, JIT provisioning, `GET /me`.                                      |
| 002. [Workspace Self-Onboarding & Membership](./tickets/002-workspace-self-onboarding-and-membership.md) | Todo   | `workspace_memberships`, SpiceDB human-management model (+ dormant `platform`), `POST`/`GET /workspaces`, member add/remove, dual-write. |
| 003. [Actor & API Key Management](./tickets/003-actor-and-api-key-management.md)                         | Todo   | Workspace-scoped actors, multi-credential rotation, `manage_actors`, create/list actors, issue/revoke keys.                              |
| 004. [Auth & Identity Audit Events](./tickets/004-auth-and-identity-audit-events.md)                     | Todo   | `audit_events.user_id`, in-transaction audit writer, emit identity events.                                                               |

## Sequencing

- **001** is foundational: every management route consumes its `UserContext`.
- **002** depends on 001 and introduces the SpiceDB human-management schema
  (`owner`/`admin`, `manage_*`) plus dormant platform-superadmin scaffolding;
  land it before 003.
- **003** depends on 002 and changes data-plane auth to support multiple
  credentials per actor.
- **004** depends on 001–003 for the operations it instruments; its writer/schema
  can be built early but event emission lands alongside 002 and 003.
