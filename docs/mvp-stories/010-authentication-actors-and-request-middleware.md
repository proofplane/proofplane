# 010 - Authentication, Actors, Request Middleware, and Authorization Boundary

## Goal

Add actor-aware authentication, request middleware, and the first authorization
boundary for the Evidence Request API.

Evidence Request endpoints now provide concrete route behavior for the first auth
slice. This story should authenticate actors and create an authorization shape
that later stories can extend without committing the MVP to a full permission
graph before more domain entities exist.

## Design

Model actors as first-class domain objects:

- human user
- AI agent
- service account
- integration
- policy automation

Use locally managed, hashed API keys for the MVP. Auth0 is not part of the current plan. If Proofplane later needs Auth0 or another external identity provider, handle that as a future migration or additional auth provider rather than shaping the MVP around it now.

API keys must be stored hashed at rest. For generated high-entropy API keys, prefer a simple deterministic keyed hash using the configured credential pepper unless later requirements call for a different scheme. Do not store raw API keys.

Protect the Evidence Request API in this story. The initial authorization policy
should be deliberately narrow: authenticated actors may access Evidence Requests
only through their own workspace, and handlers or services should call an
authorization boundary that can later be backed by an external permissions
system. Do not add a broad permission-scope model before controls, mappings,
submissions, approvals, source material, and audit reads clarify the actions
that need to be modeled.

Run a small SpiceDB design spike as part of this story:

- draft the first Proofplane authorization model for actors, workspaces, and
  Evidence Requests
- identify the relationship and permission questions expected from upcoming
  controls, submissions, approvals, source material, audit, and MCP work
- record how Proofplane domain writes would eventually keep authorization
  relationships synchronized
- update the follow-up SpiceDB authorization story with findings and open
  questions

The spike is a design artifact, not a production SpiceDB integration. The first
implementation should make the later integration straightforward by keeping
authentication, actor context, and authorization decisions explicit.

Middleware responsibilities:

- assign or propagate request ID
- authenticate actor
- attach actor context to request extensions
- authorize Evidence Request reads and writes through the authorization boundary
- log every request
- normalize error responses

## Acceptance Criteria

- Protected routes reject missing or invalid credentials.
- Authenticated requests include actor context available to handlers and services.
- Evidence Request endpoints reject cross-workspace access for authenticated actors.
- Evidence Request authorization flows through an explicit boundary that can be replaced by a SpiceDB-backed implementation later.
- Request logs include actor ID when authenticated.
- API keys are hashed at rest.
- Auth0 is not required for the MVP and is documented as deferred.
- The SpiceDB spike produces a draft model and updates the later SpiceDB story with findings and open questions.
- Authorization rules remain tied to concrete endpoints rather than a speculative full permission model.
- Seed data includes at least one actor for each relevant MVP actor type.

## Tests

- Unit tests cover credential hashing and verification.
- API tests cover missing auth, invalid auth, valid auth, and actor context propagation.
- API tests cover same-workspace Evidence Request access and cross-workspace rejection.
- Authorization boundary tests cover the initial Evidence Request read and write actions.
- Middleware tests verify logging metadata without leaking secret headers.
- Seed tests verify demo credentials produce usable actors.

## QA Guide

1. Run migrations and seed.
2. Call a protected endpoint without credentials and confirm `401`.
3. Call with seeded credentials and confirm success.
4. Call an Evidence Request path in a different workspace and confirm access is rejected.
5. Inspect logs and confirm request ID and actor ID are present but the credential is absent.
