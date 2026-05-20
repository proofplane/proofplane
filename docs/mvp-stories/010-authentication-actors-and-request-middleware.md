# 010 - Authentication, Actors, and Request Middleware

## Goal

Add actor-aware authentication and request middleware for auth, logging, request IDs, and future authorization.

This story is deferred until real API endpoints exist. Authentication and authorization rules should be designed around concrete route behavior instead of guessed ahead of the product surface.

## Design

Model actors as first-class domain objects:

- human user
- AI agent
- service account
- integration
- policy automation

Use locally managed, hashed API keys for the MVP. Auth0 is not part of the current plan. If Proofplane later needs Auth0 or another external identity provider, handle that as a future migration or additional auth provider rather than shaping the MVP around it now.

API keys must be stored hashed at rest. For generated high-entropy API keys, prefer a simple deterministic keyed hash using the configured credential pepper unless later requirements call for a different scheme. Do not store raw API keys.

Authorization is intentionally undecided. Define authorization rules when the endpoints they protect exist. Early product endpoints may remain public, local-only, or protected by temporary development controls until this story is picked up.

Middleware responsibilities:

- assign or propagate request ID
- authenticate actor
- attach actor context to request extensions
- log every request
- normalize error responses

## Acceptance Criteria

- Protected routes reject missing or invalid credentials.
- Authenticated requests include actor context available to handlers and services.
- Request logs include actor ID when authenticated.
- API keys are hashed at rest.
- Auth0 is not required for the MVP and is documented as deferred.
- Authorization rules are tied to concrete endpoints rather than placeholder role models.
- Seed data includes at least one actor for each relevant MVP actor type.

## Tests

- Unit tests cover credential hashing and verification.
- API tests cover missing auth, invalid auth, valid auth, and actor context propagation.
- Middleware tests verify logging metadata without leaking secret headers.
- Seed tests verify demo credentials produce usable actors.

## QA Guide

1. Run migrations and seed.
2. Call a protected endpoint without credentials and confirm `401`.
3. Call with seeded credentials and confirm success.
4. Inspect logs and confirm request ID and actor ID are present but the credential is absent.
