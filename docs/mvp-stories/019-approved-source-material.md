# 019 - Approved Source Material

## Goal

Create the trusted source layer that downstream questionnaire agents can query.

## Design

Approved source material contains approved facts or answer fragments linked to:

- controls
- evidence requirements
- current approved submissions
- freshness metadata
- approval actor
- approval rationale

This feature does not generate questionnaire responses. It provides approved material that customer-owned agents can use elsewhere.

## Acceptance Criteria

- Source material tables are migrated.
- API supports create or update approved material, search by topic/control, and retrieve by ID.
- Records link back to requirements, controls, and submissions where applicable.
- Staleness or expiry can be represented based on linked evidence freshness.
- Seed data includes approved source material for demo controls.

## Tests

- Domain tests cover source material validation and freshness state.
- Repository integration tests cover create/update/search/retrieve.
- API integration tests cover write, search, retrieve, invalid links, and validation errors.
- Tests verify stale linked evidence marks material stale or returns freshness metadata.
- Seed tests verify demo material is queryable.

## QA Guide

1. Run seed.
2. Search approved material by a demo control.
3. Create new approved material linked to approved evidence.
4. Retrieve it and confirm provenance links are present.
5. Expire linked evidence and confirm freshness metadata changes.
