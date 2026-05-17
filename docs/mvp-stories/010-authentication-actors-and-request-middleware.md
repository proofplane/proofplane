# 010 - Authentication, Actors, and Request Middleware

## Goal

Add actor-aware authentication and request middleware for auth, logging, request IDs, and future authorization.

## Design

Model actors as first-class domain objects:

- human user
- AI agent
- service account
- integration
- policy automation

Start with API-key authentication suitable for local MVP development. Keep the design open for OAuth or signed service credentials later.

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
