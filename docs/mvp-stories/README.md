# Proofplane MVP Story Backlog

This directory is an ordered, file-based execution plan for the Proofplane MVP. Treat each markdown file as a story inside one long-running epic.

The sequence intentionally front-loads platform scaffolding before product features:

| Story | Status | Notes |
| --- | --- | --- |
| 001. [Repository and Crate Scaffold](./001-repository-and-workspace-scaffold.md) | Done | Single-package Rust scaffold, binaries, core modules, and Makefile are in place. Integration coverage was added later under 009. |
| 002. [Local Docker Compose Dependencies](./002-local-docker-compose-dependencies.md) | Done | Docker Compose covers Postgres and the Pub/Sub emulator; local config reserves `.local/storage` for future filesystem object storage. |
| 003. [Applicative Validation Framework](./003-applicative-validation-framework.md) | Done | Generic `Validation<T, E>` API and `validate!` macro are implemented and tested. |
| 004. [Configuration System](./004-configuration-system.md) | Done | Typed YAML config loads through `PROOFPLANE_CONFIG`, validates with story 003, and redacts secrets. |
| 005. [Error, Retry, and Result Extensions](./005-error-retry-and-result-extensions.md) | Done | Shared async retry helper is in place and concrete boundary errors use `thiserror`; retry logging/metrics are deferred until real runtime needs appear. |
| 006. [Observability Scaffold](./006-observability-scaffold.md) | Done | Structured logging initializes through `tracing_subscriber`; metrics are deferred until real runtime boundaries exist. |
| 007. [HTTP API Runtime Scaffold](./007-http-api-runtime-scaffold.md) | Done | Axum API runtime serves health, readiness, version, metrics, stable errors, and structured request logs. |
| 008. [Database Migrations and Seed Data](./008-database-migrations-and-seed-data.md) | Done | Refinery migrations, Postgres pool wiring, startup migrations, and idempotent local seed data are in place. |
| 009. [Integration Test Harness](./009-integration-test-harness.md) | Done | Postgres/testcontainers API harness and Evidence Request integration coverage are in place; later stories add dependency-specific coverage as needed. |
| 010. [Authentication, Actors, Request Middleware, and SpiceDB Authorization](./010-authentication-actors-and-request-middleware.md) | Done | API-key auth, actor context, local SpiceDB schema/bootstrap, and workspace-scoped Evidence Request authorization are in place. |
| 011. [Pub/Sub Client and Push Subscription Provisioning](./011-pubsub-client-and-subscription-runtime.md) | Done | Publisher, emulator support, application topic registry, topic provisioning, worker push subscription provisioning, and dead-letter topic configuration are in place; there is intentionally no pull subscriber runtime in the MVP. |
| 012. [Transactional Outbox](./012-transactional-outbox.md) | Done | Outbox schema/repository, generic dequeuer, dequeuer binary, Pub/Sub publisher integration, retry scheduling, and emulator integration coverage are in place. |
| 013. [Pub/Sub Push Worker Handler Runtime](./013-worker-runtime-and-outbox-dequeuer.md) | Done | Worker binary, Pub/Sub push endpoint, log-only `attachment.scan_requested` dispatch, dequeuer-owned worker subscription/dead-letter provisioning, and Deltio provisioning coverage are in place; scanner/domain work remains deferred. |
| 014. [GCS Object Storage Adapter](./014-gcs-object-storage-adapter.md) | Partial | Filesystem-backed local/test object storage is implemented and used by attachment uploads; the production GCS adapter remains open. |
| 015. [Evidence Requests Domain](./015-evidence-requirements-domain.md) | Done | Evidence Request domain, migration, seed data, service, REST endpoints, and integration tests are in place. |
| 016. [Controls and Requirement Mappings](./016-controls-and-requirement-mappings.md) | Done | Control registry, SOC 2 reference data, durable Evidence Request-control mappings, authz, seed data, and integration coverage are in place. Event emission remains deferred until specific product event contracts are added. |
| 017. [Evidence Submissions and Attachments](./017-evidence-submissions-and-attachments.md) | Partial | Submission create/read, multipart attachment upload, CRC32C validation, filesystem object writes, attachment lifecycle status, scan-request outbox dispatch, scanner boundary, and idempotent scan/finalization workers are in place; download enforcement, latest-submission API, audit polish, and seed data remain open. |
| 018. [Deferred Submission Approval and Control Status](./018-submission-approval-and-control-status.md) | Deferred | Native approval is out of MVP; caller workflows own review before upload unless customer feedback changes this. |
| 019. [Approved Source Material](./019-approved-source-material.md) | Planned | Not started. |
| 020. [Audit Log](./020-audit-log.md) | Planned | Not started. |
| 021. [MCP Server](./021-mcp-server.md) | Planned | Not started. |
| 022. [Dependency Failure Integration Coverage](./022-dependency-failure-integration-coverage.md) | Partial | Concrete-Postgres attachment worker rollback/retry coverage is in place; readiness, SpiceDB, Pub/Sub, and public API storage-failure coverage remain open. |
| 023. [Prometheus Metrics Instrumentation](./023-prometheus-metrics-instrumentation.md) | Planned | Add application metrics on top of the existing `/metrics` scaffold before final demo hardening. |
| 024. [End-to-End Demo and Release Hardening](./024-end-to-end-demo-and-release-hardening.md) | Planned | Not started. |
| 025. [Marketing Site and Sandbox Onboarding](./025-marketing-site-and-sandbox-onboarding.md) | Planned | Product-led GTM milestone: public site, sandbox CTA, first-run SOC 2 flow, and AI-answer readiness. |

## Parallelization Notes

The numbered order is the preferred integration order, but several stories can be developed in parallel once their shared prerequisites are done.

Shared gates:

- Stories 001-004 are complete and unblock the next layer of platform work.
- Story 005 standardized the retry helper and concrete `thiserror` boundary errors before deeper runtime work.
- Story 009 provides the Postgres/testcontainers API harness used by the Evidence Request integration tests. Story 022 owns dependency-failure integration coverage as those boundaries become concrete.

Near-term parallel lanes:

- Story 017 is the current mainline product surface. Its submission and upload
  slices are partially complete and scan-request dispatch is wired through the
  outbox/Pub/Sub push worker path, with scanning and clean-object finalization
  split into independently retryable deliveries; the next work is scan-state
  enforcement, download enforcement, and
  latest-submission polish.
- Later stories should extend the integration harness with reusable Pub/Sub,
  object-storage, config, and binary helpers as those boundaries become shared.
- Story 016 followed story 010 with the first control and mapping entities. The auth direction is local hashed API keys; Auth0 is out of scope for the MVP unless revisited later.

Infrastructure lanes after config:

- Story 011 is complete for the MVP Pub/Sub client boundary. It owns publishing,
  application topic provisioning, and worker push subscription provisioning;
  story 013 owns the worker HTTP runtime that receives push deliveries.
- Story 012 is complete for the single-process transactional outbox dequeuer.
- Story 014's filesystem object-storage slice is in place. The GCS adapter
  remains open before production object storage is complete.
- Story 013 is complete for the MVP worker runtime. Pub/Sub push subscriptions
  are used in both live and local environments: live push targets Cloud Run,
  local push targets the same worker endpoint through Deltio, and there is no
  separate local relay process.
- Do not require an outbox for synchronous product stories just because the
  domain change might matter later. Use the transactional outbox when a product
  story truly needs durable asynchronous events or background work.

Dependency-failure hardening lane:

- Story 022 can be developed in parallel by dependency boundary. The Postgres readiness slice can start after stories 007-009 because health/readiness and the integration harness already exist.
- The SpiceDB authorization-failure slice can start after story 010, once Evidence Request authz uses the real SpiceDB adapter.
- Pub/Sub failure coverage should land alongside or after stories 011-013, once the client, outbox, push subscription, dead-letter, and worker HTTP acknowledgement behavior are in place.
- Object-storage failure coverage should land alongside or after stories 014 and 017, once attachment endpoints exercise storage through public API paths.
- Story 022 should not block product stories except where a product story introduces a new external boundary; in that case, add the corresponding failure coverage before final demo hardening.

Metrics instrumentation lane:

- Story 023 builds on the `/metrics` endpoint from story 007 and should land
  after the main API, worker, storage, and dependency surfaces have enough real
  behavior to measure.
- Product and infrastructure stories should add narrowly scoped metrics when a
  boundary naturally needs them, but story 023 is the release gate that makes
  metric names, labels, and cardinality consistent across the MVP.

Product lanes:

- Story 010 depends on 007, 008, and the Evidence Request routes from 015. It adds actor-aware API auth, local SpiceDB runtime/bootstrap, and initial workspace-scoped authorization checks.
- Story 015 is complete and provides the Evidence Request base for the main product model. Story 010 protects its endpoints.
- Story 016 is complete and depends on 010 and 015.
- Story 017 depends on 014 and 015. Its submission/upload slices already use the
  actor context from story 010 and its upload-accepted scan dispatch uses the
  outbox/Pub/Sub push path from stories 011-013. The noop scanner boundary and
  concrete-Postgres scan/finalization handlers are implemented; remaining work
  should focus on real scanner execution, attachment-state
  enforcement, latest-submission reads, and
  audit polish.
- Story 018 is deferred until customer feedback shows that Proofplane should own approval state.
- Story 019 depends on the evidence/control/submission model from 015-017, with
  usability gated by attachment upload status rather than submission approval.
- Story 020 can start its schema/repository design after 008 and 010, but service-level audit calls should be integrated alongside stories 015-019.
- Story 021 depends on the domain services created by 015-020 and should use the authentication and authorization model introduced in 010.
- Story 022 hardens dependency-failure behavior before the final release gate.
- Story 023 adds metrics instrumentation before demo hardening.
- Story 024 should remain last because it validates the complete system.
- Story 025 is a product-led GTM story after the backend MVP is demo-quality. It
  should not block the agent-native backend, but it is the main path from
  marketing interest to hands-on product usage.

Definition of done for every story:

- Dependencies are passed explicitly. Use traits and static generics for real
  swappable boundaries such as scanners, object stores, and publishers; depend
  directly on concrete internal gateways such as `repository::Postgres` instead
  of introducing mock-only repository traits.
- Async work uses `tokio`.
- Errors use `thiserror`.
- Pure parsing and domain behavior has focused unit tests. Persistence,
  transactions, worker coordination, and other infrastructure behavior belongs
  in the dedicated integration test target with concrete dependencies wherever
  practical.
- Seed data is updated whenever a story introduces user-visible or queryable data.
- The QA guide in the story can be followed from a clean checkout with local Docker dependencies.
