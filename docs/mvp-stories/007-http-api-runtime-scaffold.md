# 007 - HTTP API Runtime Scaffold

## Goal

Create the API server runtime with health probes, metrics, middleware slots, and layered request flow.

## Design

Use a Rust async HTTP framework compatible with `tokio` and tower-style middleware.

Expose:

- `GET /livez`
- `GET /readyz`
- `GET /metrics`
- version endpoint

Handlers must follow the target architecture:

```text
request DTO -> request validation -> domain type -> service -> response DTO or error DTO
```

The service layer handles domain logic, repository calls, and retrying. The repository layer handles SQL.

## Acceptance Criteria

- API binary starts with config-loaded bind address.
- `/livez` returns success when the process event loop is healthy.
- `/readyz` checks critical dependencies such as Postgres and Pub/Sub.
- Error responses use a stable JSON shape.
- Handler modules do not contain SQL.
- Service modules do not depend on HTTP request or response types.

## Tests

- Unit tests cover DTO mapping and error DTO mapping.
- API integration tests cover liveness, readiness success, readiness dependency failure, metrics, and unknown route.
- Compile-time dependency boundaries are enforced by crate layout.
- Tests verify validation failure returns all accumulated field errors.

## QA Guide

1. Start local dependencies.
2. Run the API with local config.
3. Call `/livez`, `/readyz`, and `/metrics`.
4. Stop Postgres and confirm `/readyz` fails while `/livez` still succeeds.
