# Authentication Hierarchy Spec

## Goal

Support customer self-onboarding with a three-tier identity model:

1. A human signs up and authenticates via **Auth0**.
2. That human self-onboards by **creating a workspace** (the tenant boundary),
   becoming its owner.
3. The human **creates actors** under their workspace and issues **API keys**
   for them. Those actor keys authenticate all calls to the data APIs (evidence
   requests, evidence submissions, controls).

Humans **manage**; actors **do work**. The two never cross.

## Decisions

- **User ↔ workspace is many-to-many** from day one. A human can belong to
  several workspaces; a workspace can have several human admins.
- **Clean separation of planes.** Humans authenticate via Auth0 and operate only
  the management plane. Actors authenticate via API keys and operate only the
  data plane. Humans do not call data APIs; actors do not manage anything.
- **No platform-superadmin tier now**, but the SpiceDB schema is wired so it can
  be activated later with no breaking change (see below).
- **Human roles at launch: Owner + Admin.** Owner can delete/transfer the
  workspace and do everything; Admin can manage members, actors, and keys but
  cannot delete the workspace.
- **JIT provisioning.** The `users` row is created the first time a valid Auth0
  token arrives, keyed on `sub`. No separate signup endpoint.

## Two identity types

|        | **User** (new)                           | **Actor** (exists)                          |
| ------ | ---------------------------------------- | ------------------------------------------- |
| Who    | Human owner/admin                        | Machine / programmatic                      |
| Auth   | Auth0 JWT (Bearer)                       | API key (existing `x-proofplane-*` headers) |
| Can do | Manage workspaces, members, actors, keys | Hit data APIs (evidence, controls)          |
| Cannot | Touch data APIs                          | Manage anything                             |
| Scope  | Many workspaces                          | Exactly one workspace                       |

`ActorKind::HumanUser` stays as-is — it only means "a human is operating this
key." The new `User` entity is the management-plane identity. The two concepts
are orthogonal.

## Database changes (new migration `V002`)

```sql
-- Human identities (Auth0-backed), JIT-provisioned on first valid token
CREATE TABLE users (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    auth0_sub   TEXT NOT NULL UNIQUE,
    email       TEXT,
    name        TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Many-to-many user <-> workspace with role
CREATE TABLE workspace_memberships (
    user_id      UUID NOT NULL REFERENCES users(id),
    workspace_id UUID NOT NULL REFERENCES workspaces(id),
    role         TEXT NOT NULL CHECK (role IN ('owner','admin')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, workspace_id)
);

-- Give actors a real DB home: tenant isolation no longer lives only in SpiceDB
ALTER TABLE actors ADD COLUMN workspace_id       UUID REFERENCES workspaces(id);
ALTER TABLE actors ADD COLUMN created_by_user_id UUID REFERENCES users(id);

-- Allow key rotation: multiple live credentials per actor
DROP INDEX idx_api_credentials_actor_id;          -- the UNIQUE one
ALTER TABLE api_credentials DROP CONSTRAINT api_credentials_actor_id_key;
CREATE INDEX idx_api_credentials_actor_id ON api_credentials (actor_id);
```

### Migration notes

- `actors.workspace_id` is **nullable** because the seeded **system actors** are
  not workspace-bound. Tenant actors always set it; the auth path requires it for
  non-system actor kinds.
- Relaxing the one-key-per-actor constraint requires changing
  `ApiKeyAuthenticator::authenticate` (`src/authentication/mod.rs:70`). Today it
  loads the actor's single credential then checks `key_id`. With multiple
  credentials it must resolve the credential **by `key_id`** (extracted from the
  raw key) scoped to the actor.

## SpiceDB schema (`authz/spicedb/proofplane.zed`)

```
definition user {}

definition platform {
    relation super_admin: user
}

definition workspace {
    relation platform: platform     // wired now, no tuples written yet
    relation owner: user
    relation admin: user
    relation member: actor

    // Management plane (humans). platform->super_admin is dormant until the
    // platform-superadmin tier is activated later; including it now is a no-op,
    // not a breaking change.
    permission manage_workspace = owner + platform->super_admin
    permission manage_members   = owner + admin + platform->super_admin
    permission manage_actors    = owner + admin + platform->super_admin

    // Data plane (actors) — unchanged behavior
    permission read_evidence_requests     = member
    permission write_evidence_requests    = member
    permission read_evidence_submissions  = member
    permission write_evidence_submissions = member
    permission read_controls              = member
    permission write_controls             = member
}
```

The `platform` definition and the `platform->super_admin` arrows are inert until
a single `platform:proofplane#super_admin@user:X` tuple is created someday. That
is the entire "flexibility for later" — no schema rework when the tier is
activated.

## Two auth middleware paths

- **Existing** `authorize_workspace_route` (`src/routes/authentication.rs:31`) —
  unchanged. Guards data routes via API key, producing `ActorContext`.
- **New** `authenticate_user` — validates the Auth0 Bearer JWT (RS256, verified
  against a cached JWKS, with `iss` / `aud` / `exp` checks), JIT-provisions the
  `users` row from `sub`, and produces a `UserContext { user_id, auth0_sub }`.
  Management routes then check SpiceDB `manage_*` permissions.

Do not hand-roll JWKS fetching, key rotation, or signature verification. Use a
focused, framework-agnostic JWKS+JWT crate (recommended: `jwtk`, which fetches and
caches the remote JWKS with `kid` rotation and verifies RS256; fallback
`jsonwebtoken` + a small JWKS cache if the crate's maintenance/version does not
check out at build time). Avoid a full Axum auth-*layer* crate (e.g.
`jwt-authorizer`): it leaks its layer/claims types into the router against this
project's adapter/DI convention, and cannot perform JIT provisioning anyway. Wrap
the chosen crate behind a `TokenVerifier` trait (static DI), mirroring how SpiceDB
sits behind `WorkspaceAuthorizer` and API keys behind `ApiKeyManager`; the trait
boundary also lets tests inject a fake verifier instead of calling live Auth0. No
`reqwest` dependency is needed. Auth0 config (domain, audience) goes in
`AppConfig`.

## New management endpoints (Auth0-authenticated, account-level)

```
POST   /workspaces                                              create + auto-own (self-onboard)
GET    /workspaces                                              list mine
POST   /workspaces/{id}/members                                 manage_members
DELETE /workspaces/{id}/members/{user_id}                       manage_members
POST   /workspaces/{id}/actors                                  manage_actors
GET    /workspaces/{id}/actors                                  manage_actors
POST   /workspaces/{id}/actors/{actor_id}/credentials          issue key (raw key shown ONCE)
DELETE /workspaces/{id}/actors/{actor_id}/credentials/{cred_id} revoke
```

`POST /workspaces` is the bootstrap: it is authenticated by user identity but is
**not** workspace-scoped, and it atomically makes the creator the `owner`. This
resolves the chicken-and-egg problem where every existing route assumes a
workspace in the path.

## Dual-write consistency (Postgres ↔ SpiceDB)

Reuses the existing `outbox_messages` table and its dequeuer/worker. For every
operation that writes both Postgres and SpiceDB (create workspace + owner, add
member, create actor):

1. In **one PG transaction**, write the row(s) **and** an `outbox_messages` row
   describing the SpiceDB tuple.
2. After commit, make a best-effort **synchronous** SpiceDB write (idempotent
   `touch`) so the owner can use the workspace immediately.
3. The existing **worker** drains the outbox and retries the SpiceDB write if the
   synchronous attempt failed.

This gives snappy UX with a guaranteed eventually-consistent backstop, reusing
infrastructure that already exists.

## Build order

1. **User identity + Auth0 middleware** — `users` table, JWKS validation,
   `UserContext`, JIT provisioning.
2. **Workspace self-onboarding** — `workspace_memberships` table, SpiceDB
   `user` / `owner` / `admin` relations, `POST` / `GET /workspaces`, outbox
   dual-write.
3. **Actor management** — `actors.workspace_id`, create actor, issue/revoke
   keys, multi-credential auth change.
4. **Management permission wiring** + platform-superadmin scaffolding in the
   `.zed` schema.
5. **Audit events** — start populating the currently dormant `audit_events`
   table for login / workspace / member / key events.

## Concerns addressed

- **"Root" naming** — the customer-facing concept is a workspace **owner**, not a
  cross-tenant superuser. "Root"/superadmin is reserved for a future platform
  tier (scaffolded, dormant).
- **Tenant isolation in DB** — actors gain a real `workspace_id` so isolation is
  defense-in-depth, not SpiceDB-only.
- **Bootstrap gap** — solved by the account-level `POST /workspaces`.
- **Two identity types** — clean separation of management vs data planes.
- **Cardinality** — many-to-many from day one.
- **Dual-write** — outbox + synchronous write-through.
- **Key rotation** — one-key-per-actor constraint relaxed.
