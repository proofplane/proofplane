# 015 - Evidence Requirements Domain

## Goal

Implement evidence requirement management through API and service layers.

## Design

Evidence requirements are durable objects describing what evidence is needed. Include:

- title
- description
- expected evidence type
- collection instructions
- owner actor or team reference
- cadence
- due date
- source system
- freshness or expiry rule
- status
- workspace ID

API endpoints should support create, list, get, update, list due, and list stale. Request handlers validate DTOs with the applicative validation macro, map to domain types, invoke services, and map results to response DTOs.

## Acceptance Criteria

- Requirement table is migrated.
- Repository supports create, get, list, update, list due, and list stale.
- Service enforces domain invariants and uses retry helpers for repository errors where retries are appropriate.
- API exposes requirement endpoints.
- Seed data includes realistic demo requirements.
- Handlers contain no SQL and services contain no HTTP DTOs.

## Tests

- Domain unit tests cover parsing and invariants.
- Validation tests cover invalid request payloads and accumulated field errors.
- Repository integration tests cover CRUD and due/stale queries.
- API integration tests cover create, list, get, update, due, stale, auth, and validation failures.
- Seed tests verify demo requirements exist.

## QA Guide

1. Run migrations and seed.
2. Start API.
3. List seeded requirements.
4. Create a requirement with multiple invalid fields and confirm all field errors return.
5. Create a valid requirement and retrieve it by ID.
