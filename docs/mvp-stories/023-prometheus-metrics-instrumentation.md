# 023 - Prometheus Metrics Instrumentation

## Goal

Instrument Proofplane's API, worker, and dependency boundaries with stable
Prometheus metrics that are useful for local debugging, demo readiness, and
production operations.

The `/metrics` endpoint and Prometheus recorder are already scaffolded. This
story adds the application-specific counters, histograms, and gauges emitted by
real runtime paths.

## Design

Use the existing `metrics` and `metrics-exporter-prometheus` crates. Metrics
names must use a stable `proofplane_` prefix and low-cardinality labels.

Start with API metrics:

- HTTP request count by matched route, method, and status class
- HTTP request duration by matched route and method
- API in-flight request gauge
- readiness dependency status for Postgres and other configured dependencies
- authentication result counters
- authorization result counters by permission and result
- service operation counters and duration histograms for Evidence Requests,
  controls, submissions, and attachments as those services exist
- repository operation error counters where they add operational signal without
  duplicating service metrics

Extend metrics as later boundaries become concrete:

- Pub/Sub publish, receive, ack, nack, retry, and dead-letter counters
- outbox claim, publish, retry, failed, and backlog gauges
- worker handler duration, success, and failure counters
- object storage put, get, delete, byte, checksum, and failure metrics
- structured audit-log emission counters once audit logging exists

Avoid high-cardinality labels. Do not label with workspace ID, actor ID, request
ID, raw path parameters, object keys, evidence request IDs, control IDs, API key
IDs, or error strings. Prefer route patterns, operation names, dependency names,
status classes, and coarse result labels.

## Acceptance Criteria

- `/metrics` exposes stable `proofplane_` metric names after normal API traffic.
- HTTP metrics use matched route patterns rather than raw request paths.
- Authentication and authorization metrics distinguish allowed, denied,
  unauthenticated, and dependency-error outcomes without leaking identifiers.
- Dependency health metrics expose current readiness status for implemented
  dependencies.
- Worker, Pub/Sub, outbox, object-storage, and audit metrics are added when
  those runtime surfaces exist; any unavailable surfaces are explicitly deferred
  in this story's implementation notes.
- Metric labels are documented and reviewed for cardinality risk.
- Existing logs remain responsible for request-specific investigation; metrics
  remain aggregate.

## Tests

- Unit tests cover any metric naming/label helper functions.
- API integration tests make representative requests and assert `/metrics`
  contains the expected `proofplane_` metric names and low-cardinality labels.
- Tests verify raw workspace IDs, actor IDs, request IDs, API keys, and object
  keys do not appear in emitted metric labels.
- Worker/Pub/Sub/outbox/object-storage metric tests land alongside those
  implementations when the corresponding surfaces exist.

## QA Guide

1. Start local dependencies.
2. Run migrations, seed data, and the API.
3. Exercise health, authentication failure, authorization denial, Evidence
   Request, controls, submission, and attachment flows that exist at that point
   in the MVP.
4. Call `/metrics` and confirm stable `proofplane_` metrics are present.
5. Confirm metric labels use route patterns and coarse result labels, not raw
   identifiers or secrets.
6. Start the worker after stories 011-013 land and confirm worker/outbox metrics
   are exposed.
