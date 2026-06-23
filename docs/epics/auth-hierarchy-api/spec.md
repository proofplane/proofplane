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
- **One authorization store: Postgres.** The **human management plane**
  authorizes from `workspace_memberships.role`; the **actor data plane**
  authorizes from `actors.workspace_id` + `actor_permissions`. _(Revised: ticket
  002 moved the human plane off SpiceDB; ticket 003 removed SpiceDB entirely and
  ported the actor permissions to Postgres. The original draft used SpiceDB as
  the actor data-plane engine with an outbox dual-write — both are gone.)_
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

-- Give actors a real DB home (NOT NULL: every actor belongs to one workspace).
-- Existing rows are backfilled to a dedicated system workspace before the
-- constraint is enforced.
ALTER TABLE actors ADD COLUMN workspace_id       UUID REFERENCES workspaces(id);
ALTER TABLE actors ADD COLUMN created_by_user_id UUID REFERENCES users(id);
-- ... backfill, then ...
ALTER TABLE actors ALTER COLUMN workspace_id SET NOT NULL;

-- Allow key rotation: multiple live credentials per actor
DROP INDEX idx_api_credentials_actor_id;          -- the UNIQUE one
ALTER TABLE api_credentials DROP CONSTRAINT api_credentials_actor_id_key;
CREATE INDEX idx_api_credentials_actor_id ON api_credentials (actor_id);

-- Per-actor data-plane permission grants (replaces the SpiceDB engine).
CREATE TABLE actor_permissions (
    actor_id   UUID NOT NULL REFERENCES actors(id) ON DELETE CASCADE,
    permission TEXT NOT NULL CHECK (permission IN (
        'read_evidence_requests', 'write_evidence_requests',
        'read_evidence_submissions', 'write_evidence_submissions',
        'read_controls', 'write_controls')),
    PRIMARY KEY (actor_id, permission)
);
```

### Migration notes (revised — ticket 003)

- `actors.workspace_id` is **NOT NULL**. The original draft made it nullable for
  un-bound system actors; instead, a dedicated system workspace is seeded and the
  system actor is added to it, so every actor belongs to exactly one workspace.
- Relaxing the one-key-per-actor constraint changed
  `ApiKeyAuthenticator::authenticate`: it resolves the credential **by `key_id`**
  (extracted from the raw key) scoped to the actor, and returns the actor's home
  workspace + permission grants on `ActorContext`.
- The data-plane guard (`authorize_workspace_route`) then enforces that the
  actor's home workspace equals the path workspace (404 on mismatch), and each
  resource guard checks the specific `actor_permissions` grant for the
  route+method (404 when not granted).

## Data-plane authorization (revised — ticket 003: SpiceDB removed)

The original draft authorized the actor data plane through SpiceDB
(`workspace#member@actor`, where `member` granted all six permissions). **Ticket
003 removed SpiceDB entirely** — the gRPC client, the proto build pipeline, the
`.zed` schema, the config, and the local/test infra are all gone. The six
permissions it modelled were preserved, not collapsed:

- An actor belongs to exactly one workspace (`actors.workspace_id`).
- An actor holds an explicit subset of the six data-plane permissions
  (`actor_permissions`), specified at create time.
- A data-plane request is authorized iff the actor's workspace matches the path
  **and** the actor holds the permission for that route+method (GET → the
  matching `read_*`, POST/PUT/DELETE → the matching `write_*`). Either failure is
  a 404, so existence is not leaked.

This keeps a single Postgres source of truth and removes the dual-store
reconciliation the SpiceDB engine required.

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
`workspace_memberships.role` (`owner` or `admin`). _(At the time this was written,
SpiceDB was kept as the actor data-plane engine; **ticket 003 later removed it**
in favor of Postgres `actors.workspace_id` + `actor_permissions` — see
[Data-plane authorization](#data-plane-authorization-revised--ticket-003-spicedb-removed).)_
**Revisit** an external relationship engine only if authorization ever needs
genuinely relational, reverse-queryable access (team hierarchies, org→workspace
inheritance, the platform tier).

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
the chosen crate behind a `TokenVerifier` trait (static DI), mirroring how API
keys sit behind `ApiKeyManager`; the trait boundary also lets tests inject a fake
verifier instead of calling live Auth0. No
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

## Dual-write consistency (Postgres ↔ SpiceDB) — obsolete (ticket 003)

This section described keeping SpiceDB in sync with Postgres when creating an
actor (write the row + an `outbox_messages` row for the tuple, plus a best-effort
synchronous tuple write, with the worker as backstop). **It no longer applies:**
SpiceDB was removed, so creating an actor is a single Postgres transaction
(actor row + its `actor_permissions` rows) with no second store to reconcile.

The `outbox_messages` table, dequeuer, and worker remain — they are used by the
attachment virus-scanning pipeline (Pub/Sub), which is unrelated to actor
authorization.

## Build order

1. **User identity + Auth0 middleware** — `users` table, JWKS validation,
   `UserContext`, JIT provisioning.
2. **Workspace self-onboarding** — `workspace_memberships` table, `POST` /
   `GET /workspaces` (workspace + owner membership committed in one Postgres
   transaction), member add/remove. Human `manage_*` authorized from Postgres.
3. **Actor management** — `actors.workspace_id` (NOT NULL) + `actor_permissions`,
   create/list actors, issue/revoke keys, multi-credential auth change, and
   removal of SpiceDB (data-plane authz moves to Postgres).
4. **Audit logs** — emit structured `type = "audit_log"` application logs for
   explicit `POST /login`, workspace, member, and API-token operations after
   successful commits. `POST /login` updates `users.last_login_at` on every
   successful login; `GET /me` remains a profile read.

## Concerns addressed

- **"Root" naming** — the customer-facing concept is a workspace **owner**, not a
  cross-tenant superuser. "Root"/superadmin is reserved for a future platform
  tier (deferred; not built or scaffolded).
- **Tenant isolation in DB** — actors have a NOT NULL `workspace_id`, so
  isolation lives in Postgres.
- **Bootstrap gap** — solved by the account-level `POST /workspaces`.
- **Two identity types** — clean separation of management vs data planes, both
  authorized from Postgres: human roles in `workspace_memberships`, actor access
  from `actors.workspace_id` + `actor_permissions`.
- **Cardinality** — users ↔ workspaces is many-to-many; an actor belongs to
  exactly one workspace.
- **No dual-write** — SpiceDB removed, so every write is a single Postgres
  transaction.
- **Key rotation** — one-key-per-actor constraint relaxed.
