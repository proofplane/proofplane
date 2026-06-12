# 007 - HTTP API Runtime Scaffold

## Goal

Create the API server runtime with health probes, metrics, middleware slots, and layered request flow.

## Design

Use `tokio`, `axum`, and `tower-http` for HTTP.

Expose:

- `GET /livez`
- `GET /readyz`
- `GET /metrics`
- `GET /version`

Handlers must follow the target architecture:

```text
request DTO -> request validation -> domain type -> service -> response DTO or error DTO
```

The service layer handles domain logic, repository calls, and retrying. The repository layer handles SQL.

For this scaffold, `/readyz` checks Postgres through the shared pool. Pub/Sub readiness is deferred until story 011 introduces a real Pub/Sub client.

## Acceptance Criteria

- API binary starts with config-loaded bind address.
- `/livez` returns success when the process event loop is healthy.
- `/readyz` checks Postgres and returns unavailable when Postgres cannot be reached.
- Error responses use a stable JSON shape.
- Handler modules do not contain SQL.
- Service modules do not depend on HTTP request or response types.
- HTTP requests logged at INFO level and include path, method, response status code, and response time
- `/metrics` returns Prometheus text; application metrics added later use stable
  `proof_` names.

## Tests

- Unit tests cover DTO mapping and error DTO mapping.
- API integration tests for liveness, readiness success, metrics, version, unknown route, and structured request logs are deferred to story 009.
- Unit tests cover response shaping where practical until the integration harness exists.
- Compile-time dependency boundaries are enforced by crate layout.

## QA Guide

1. Start local dependencies.
2. Run the API with local config.
3. Call `/livez`, `/readyz`, `/version`, and `/metrics`.
4. Inspect structured request logs for method, path, status, and latency.
5. Stop Postgres and confirm `/readyz` fails while `/livez` still succeeds.
