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
- **Two authorization stores, by plane.** The **human management plane**
  authorizes from **Postgres** (`workspace_memberships.role`), the transactional
  source of truth for human roles. **SpiceDB is the actor data plane only**
  (`workspace#member@actor` → evidence/controls). _(Revised during ticket 002 —
  see [Decision revision](#decision-revision-human-plane-authorizes-from-postgres)
  below. The original draft modelled human `owner`/`admin`/`manage_*` in SpiceDB
  with an outbox dual-write; that was dropped as redundant.)_
- **No platform-superadmin tier.** Reserved for a future platform plane; not
  built and not scaffolded (avoids dead code until it is actually needed).
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

SpiceDB models the **actor data plane only**. Human management roles are not in
SpiceDB — they are authorized from Postgres (see the decision revision below).

```
definition actor {}

definition workspace {
    relation member: actor

    // Data plane (actors)
    permission read_evidence_requests     = member
    permission write_evidence_requests    = member
    permission read_evidence_submissions  = member
    permission write_evidence_submissions = member
    permission read_controls              = member
    permission write_controls             = member
}
```

### Decision revision: human plane authorizes from Postgres

The original draft of this spec put human `owner`/`admin` relations and
`manage_workspace`/`manage_members`/`manage_actors` permissions in SpiceDB (plus
a dormant `platform` superadmin tier), kept in sync with Postgres via an outbox
dual-write and a worker backstop. **Ticket 002 dropped that.** Reasons:

- Human membership's source of truth already *is* `workspace_memberships` in
  Postgres, written transactionally. A SpiceDB projection of it added a second
  store that has to be reconciled and a read-your-writes gap (create a workspace,
  then immediately manage it → 404 until the projection catches up).
- The `manage_*` permissions were flat unions (`owner`, `owner + admin`) — nothing
  SpiceDB's relationship graph was needed for. They are a Postgres role check.
- Removing it deleted the synchronous write-through, the membership outbox events,
  and the worker reconciliation handler — two places calling SpiceDB to do the
  same thing collapsed to one transactional Postgres write.

So `manage_members` / `manage_workspace` / `manage_actors` are answered by reading
`workspace_memberships.role` (`owner` or `admin`). SpiceDB stays the data-plane
engine, where fine-grained, relational, reverse-queryable actor access is actually
heading. **Revisit** if the human plane ever needs relational authorization (team
hierarchies, org→workspace inheritance, the platform tier) — then modelling humans
in SpiceDB earns its keep.

## Two auth middleware paths

- **Existing** `authorize_workspace_route` (`src/routes/authentication.rs:31`) —
  unchanged. Guards data routes via API key, producing `ActorContext`.
- **New** `authenticate_user` — validates the Auth0 Bearer JWT (RS256, verified
  against a cached JWKS, with `iss` / `aud` / `exp` checks), JIT-provisions the
  `users` row from `sub`, and produces a `UserContext { user_id, auth0_sub }`.
  Management routes then check the caller's role in `workspace_memberships`
  (Postgres) — `owner`/`admin` grant `manage_*`. A caller without the required
  role gets **404** (indistinguishable from an absent workspace, so existence is
  not leaked).

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
DELETE /workspaces/{id}/members/{user_id}                       manage_members
POST   /workspaces/{id}/actors                                  manage_actors
GET    /workspaces/{id}/actors                                  manage_actors
POST   /workspaces/{id}/actors/{actor_id}/credentials          issue key (raw key shown ONCE)
DELETE /workspaces/{id}/actors/{actor_id}/credentials/{cred_id} revoke
```

_(Revised after ticket 002 — `POST /workspaces/{id}/members` was dropped. It added
a member by internal `user_id`, which no caller can obtain and which cannot
onboard anyone who has not already logged in — an unusable flow. A realistic
invite-by-email onboarding flow is the intended replacement but is not yet scoped;
`DELETE .../members/{user_id}` (remove-member) stays. The membership-insert
capability the endpoint wrapped is retained for that future flow.)_

`POST /workspaces` is the bootstrap: it is authenticated by user identity but is
**not** workspace-scoped, and it atomically makes the creator the `owner`. This
resolves the chicken-and-egg problem where every existing route assumes a
workspace in the path.

## Dual-write consistency (Postgres ↔ SpiceDB)

This applies **only to operations that write a SpiceDB tuple** — i.e. the actor
data plane: granting an actor `workspace#member` (ticket 003, *Actor & API Key
Management*). It does **not** apply to workspace creation or human membership:
those write only Postgres (`workspaces` + `workspace_memberships` in one
transaction) and authorize from Postgres, so there is no second store to keep in
sync.

For an operation that does write both Postgres and SpiceDB (create actor +
membership tuple), reuse the existing `outbox_messages` table and its
dequeuer/worker:

1. In **one PG transaction**, write the row(s) **and** an `outbox_messages` row
   describing the SpiceDB tuple.
2. After commit, optionally make a best-effort **synchronous** SpiceDB write
   (idempotent `touch`) so the actor can be used immediately.
3. The existing **worker** drains the outbox and retries the SpiceDB write if the
   synchronous attempt failed.

This gives snappy UX with a guaranteed eventually-consistent backstop, reusing
infrastructure that already exists.

## Build order

1. **User identity + Auth0 middleware** — `users` table, JWKS validation,
   `UserContext`, JIT provisioning.
2. **Workspace self-onboarding** — `workspace_memberships` table, `POST` /
   `GET /workspaces` (workspace + owner membership committed in one Postgres
   transaction), member add/remove. Human `manage_*` authorized from Postgres.
3. **Actor management** — `actors.workspace_id`, create actor (with SpiceDB
   `member` tuple via the outbox dual-write), issue/revoke keys, multi-credential
   auth change.
4. **Audit events** — start populating the currently dormant `audit_events`
   table for login / workspace / member / key events.

## Concerns addressed

- **"Root" naming** — the customer-facing concept is a workspace **owner**, not a
  cross-tenant superuser. "Root"/superadmin is reserved for a future platform
  tier (deferred; not built or scaffolded).
- **Tenant isolation in DB** — actors gain a real `workspace_id` so isolation is
  defense-in-depth, not SpiceDB-only.
- **Bootstrap gap** — solved by the account-level `POST /workspaces`.
- **Two identity types** — clean separation of management vs data planes, with a
  store per plane: human roles in Postgres, actor membership in SpiceDB.
- **Cardinality** — many-to-many from day one.
- **Dual-write** — outbox + synchronous write-through, for actor data-plane tuples
  only (human membership is a single transactional Postgres write).
- **Key rotation** — one-key-per-actor constraint relaxed.
