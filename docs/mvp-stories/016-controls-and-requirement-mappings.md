# 016 - Controls and Evidence Request Mappings

## Goal

Add the control registry and durable mappings from Evidence Requests to controls.

## Design

Start with SOC 2 controls. Model:

- frameworks
- controls
- Evidence Request-control mappings
- mapping rationale
- mapping approval status if needed for MVP

Mappings live on Evidence Requests, not evidence submissions. Recurring submissions inherit mappings through their Evidence Request.

## Acceptance Criteria

- Control and mapping tables are migrated.
- Seed data includes an initial SOC 2 control set sufficient for demo flows.
- API supports listing controls, getting a control, mapping an Evidence Request to a control, removing a mapping, and listing mappings.
- Service prevents duplicate mappings and validates workspace ownership.
- Mapping operations emit outbox events.

## Tests

- Domain tests cover control identifiers and mapping invariants.
- Repository integration tests cover controls and mappings.
- API integration tests cover list/get/map/remove/list mappings.
- Tests verify duplicate mappings return a stable conflict error.
- Seed tests verify SOC 2 demo controls and mappings exist.

## QA Guide

1. Run seed.
2. List controls.
3. Create an Evidence Request-control mapping with rationale.
4. Attempt the same mapping again and confirm a conflict response.
5. Remove the mapping and confirm it no longer appears.
