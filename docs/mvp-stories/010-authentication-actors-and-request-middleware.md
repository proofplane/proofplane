# 010 - Authentication, Actors, Request Middleware, and SpiceDB Authorization

## Goal

Add actor-aware authentication, request middleware, and SpiceDB-backed
authorization for the Evidence Request API.

Evidence Request endpoints now provide concrete route behavior for the first auth
slice. Integrate SpiceDB while the authorization graph is still small, then grow
the schema and relationship writes with later domain stories.

## Design

Model actors as first-class domain objects:

- human user
- AI agent
- service account
- integration
- policy automation

Use locally managed, hashed API keys for the MVP. Auth0 is not part of the
current plan. If Proofplane later needs Auth0 or another external identity
provider, handle that as a future migration or additional auth provider rather
than shaping the MVP around it now.

API keys must be stored hashed at rest. For generated high-entropy API keys,
prefer a simple deterministic keyed hash using the configured credential pepper
unless later requirements call for a different scheme. Do not store raw API
keys.

Protect the Evidence Request API in this story. The initial authorization policy
is deliberately narrow: authenticated actors may read and write Evidence
Requests only through workspaces where SpiceDB grants the matching workspace
permission. Keep Proofplane's authorization calls behind an adapter so route and
service code does not depend on generated SpiceDB types directly.

### SpiceDB Integration

SpiceDB schema and relationship data have different jobs:

- the authored `.zed` schema defines subject and resource types, relations that
  may be written, and permissions computed from those relations
- relationships are live authorization data written through SpiceDB APIs or
  tooling and stored in SpiceDB's datastore
- permission checks combine the applied schema and stored relationships to
  decide whether a subject may perform an action on a resource

This is analogous to schema versus rows in the application database. The `.zed`
file does not hold production relationship records. Later domain writes that
create or change authorization-relevant relationships must write or synchronize
those records explicitly.

The first authored schema should live at `authz/spicedb/proofplane.zed`. It
models:

- `actor` as the initial subject type
- `workspace` as the initial authorized resource type
- an actor-to-workspace membership relation
- workspace permissions for Evidence Request reads and writes

Proofplane writes membership relationships to the relation declared by the
schema, such as:

```text
workspace:00000000-0000-4000-8000-000000000001#member@actor:system-actor
```

Proofplane checks the computed workspace permissions when handling Evidence
Request reads and writes. Do not add per-Evidence-Request relationships in this
story; current Evidence Request queries are already workspace-scoped, and
resource-specific relationships should arrive when later behavior needs them.

Run SpiceDB locally from Docker Compose. Reuse the existing local Postgres
service, but provision a separate SpiceDB database so SpiceDB owns its own
datastore migrations and tables. Proofplane should use generated Tonic bindings
from pinned Authzed protobuf definitions to call the SpiceDB gRPC API.

API startup owns the first bootstrap flow:

- connect to SpiceDB
- apply or ensure the authored schema
- idempotently synchronize existing Postgres actor-to-workspace memberships into
  SpiceDB relationships

This startup sync is sufficient while actors and memberships are seeded
maintenance data. When a later story adds runtime actor or membership writes,
that write path must keep SpiceDB relationships in sync.

### Implementation Slices

#### Slice 1 - Local SpiceDB Foundation

Add the local SpiceDB dependency:

- Docker Compose service and separate Postgres-backed SpiceDB database
- SpiceDB datastore migration/bootstrap flow
- typed SpiceDB config, readiness checks, and local dependency docs
- generated gRPC client build path from pinned Authzed protobuf definitions

This slice is done when local SpiceDB starts and Proofplane can construct the
generated client.

#### Slice 2 - Schema and Relationship Bootstrap

Make Proofplane own the initial authorization model:

- add `authz/spicedb/proofplane.zed`
- apply the schema at API startup
- synchronize Postgres actor-to-workspace memberships into SpiceDB
  idempotently
- test repeated schema and relationship bootstrap

This slice is done when repeated API startup leaves the schema and seeded
workspace memberships usable for permission checks.

#### Slice 3 - API-Key Authentication and Request Context

Identify the caller:

- add real API-key hashing and verification
- resolve credentials to actor context
- seed one actor for each MVP actor type and one documented fixture API key
- assign or propagate request IDs and include actor/request context in logs
- map missing or invalid credentials to `401`

This slice is done when protected routes can distinguish authenticated and
unauthenticated callers.

#### Slice 4 - Evidence Request Authorization

Protect the first product surface:

- require API keys only for Evidence Request routes
- call SpiceDB workspace read/write permissions through the authorization
  adapter
- conceal authenticated cross-workspace Evidence Request access with `404`
- update fixtures and integration coverage

This slice is done when the seeded actor can use same-workspace Evidence Request
routes and cross-workspace requests are denied.

Middleware responsibilities:

- assign or propagate request ID
- authenticate actor
- attach actor context to request extensions
- authorize Evidence Request reads and writes through SpiceDB permissions
- log every request
- normalize error responses

## Acceptance Criteria

- Protected routes reject missing or invalid credentials.
- Authenticated requests include actor context available to handlers and services.
- Local dependencies start a usable SpiceDB service backed by its own database on
  the local Postgres service.
- Proofplane owns and applies the initial SpiceDB `.zed` schema.
- API startup idempotently synchronizes seeded actor-to-workspace membership
  relationships into SpiceDB.
- Evidence Request endpoints reject cross-workspace access for authenticated actors.
- Evidence Request authorization uses SpiceDB workspace permission checks through
  an explicit Proofplane authorization adapter.
- Request logs include actor ID when authenticated.
- API keys are hashed at rest.
- Auth0 is not required for the MVP and is documented as deferred.
- Authorization rules remain tied to concrete endpoints rather than a
  speculative full permission model.
- Seed data includes at least one actor for each relevant MVP actor type.

## Tests

- Unit tests cover credential hashing and verification.
- API tests cover missing auth, invalid auth, valid auth, and actor context propagation.
- API tests cover same-workspace Evidence Request access and cross-workspace rejection.
- SpiceDB tests cover schema/bootstrap idempotency and allowed/denied workspace
  permission checks.
- Authorization adapter tests cover the initial Evidence Request read and write
  actions.
- Middleware tests verify logging metadata without leaking secret headers.
- Seed tests verify demo credentials produce usable actors.

## QA Guide

1. Start local dependencies and confirm SpiceDB is ready.
2. Run migrations and seed.
3. Start API and confirm it applies the schema and membership relationships.
4. Call a protected endpoint without credentials and confirm `401`.
5. Call with seeded credentials and confirm success.
6. Call an Evidence Request path in a different workspace and confirm access is rejected.
7. Inspect logs and confirm request ID and actor ID are present but the credential is absent.
