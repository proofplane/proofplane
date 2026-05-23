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
| 010. [Authentication, Actors, Request Middleware, and SpiceDB Authorization](./010-authentication-actors-and-request-middleware.md) | Next | Add API-key auth and local SpiceDB-backed workspace authorization for Evidence Request routes in slices. |
| 011. [Pub/Sub Client and Subscription Runtime](./011-pubsub-client-and-subscription-runtime.md) | Planned | Not started. |
| 012. [Transactional Outbox](./012-transactional-outbox.md) | Planned | Not started. |
| 013. [Worker Runtime and Outbox Dequeuer](./013-worker-runtime-and-outbox-dequeuer.md) | Planned | Not started. |
| 014. [GCS Object Storage Adapter](./014-gcs-object-storage-adapter.md) | Planned | Not started. |
| 015. [Evidence Requests Domain](./015-evidence-requirements-domain.md) | Done | Evidence Request domain, migration, seed data, service, REST endpoints, and integration tests are in place. |
| 016. [Controls and Requirement Mappings](./016-controls-and-requirement-mappings.md) | Planned | Add the control registry and durable Evidence Request-control mappings after the first auth boundary. |
| 017. [Evidence Submissions and Attachments](./017-evidence-submissions-and-attachments.md) | Planned | Not started. |
| 018. [Submission Approval and Control Status](./018-submission-approval-and-control-status.md) | Planned | Not started. |
| 019. [Approved Source Material](./019-approved-source-material.md) | Planned | Not started. |
| 020. [Audit Log](./020-audit-log.md) | Planned | Not started. |
| 021. [MCP Server](./021-mcp-server.md) | Planned | Not started. |
| 022. [Dependency Failure Integration Coverage](./022-dependency-failure-integration-coverage.md) | Planned | Add integration coverage for readiness and fail-closed dependency behavior before release hardening. |
| 023. [End-to-End Demo and Release Hardening](./023-end-to-end-demo-and-release-hardening.md) | Planned | Not started. |

## Parallelization Notes

The numbered order is the preferred integration order, but several stories can be developed in parallel once their shared prerequisites are done.

Shared gates:

- Stories 001-004 are complete and unblock the next layer of platform work.
- Story 005 standardized the retry helper and concrete `thiserror` boundary errors before deeper runtime work.
- Story 009 provides the Postgres/testcontainers API harness used by the Evidence Request integration tests. Story 022 owns dependency-failure integration coverage as those boundaries become concrete.

Near-term parallel lanes:

- Story 010 is the next mainline task now that Evidence Request endpoints give authentication and the first SpiceDB authorization checks a concrete API surface.
- Later stories should extend the integration harness with reusable Pub/Sub, object-storage, config, and binary helpers when those boundaries exist.
- Story 016 follows story 010 with the first control and mapping entities. The auth direction is local hashed API keys; Auth0 is out of scope for the MVP unless revisited later.

Infrastructure lanes after config:

- Story 011 and story 014 can proceed independently after 004, because Pub/Sub and object storage touch different modules.
- Story 012 depends on 008 for schema and should use 005/011 where appropriate, but its repository and state-machine work can be developed before the worker exists.
- Story 013 depends on 011 and 012, so it should wait for those interfaces to settle.

Dependency-failure hardening lane:

- Story 022 can be developed in parallel by dependency boundary. The Postgres readiness slice can start after stories 007-009 because health/readiness and the integration harness already exist.
- The SpiceDB authorization-failure slice can start after story 010, once Evidence Request authz uses the real SpiceDB adapter.
- Pub/Sub failure coverage should land alongside or after stories 011-013, once the client, outbox, and worker runtime define the observable retry and acknowledgement behavior.
- Object-storage failure coverage should land alongside or after stories 014 and 017, once attachment endpoints exercise storage through public API paths.
- Story 022 should not block product stories except where a product story introduces a new external boundary; in that case, add the corresponding failure coverage before story 023.

Product lanes:

- Story 010 depends on 007, 008, and the Evidence Request routes from 015. It adds actor-aware API auth, local SpiceDB runtime/bootstrap, and initial workspace-scoped authorization checks.
- Story 015 is complete and provides the Evidence Request base for the main product model. Story 010 protects its endpoints.
- Story 016 depends on 010 and 015.
- Story 017 depends on 014 and 015, and should introduce or coordinate with story 010 when submitter actor context becomes necessary.
- Story 018 depends on 016 and 017.
- Story 019 depends on the evidence/control/submission model from 015-018.
- Story 020 can start its schema/repository design after 008 and 010, but service-level audit calls should be integrated alongside stories 015-019.
- Story 021 depends on the domain services created by 015-020 and should use the authentication and authorization model introduced in 010.
- Story 022 hardens dependency-failure behavior before the final release gate.
- Story 023 should remain last because it validates the complete system.

Definition of done for every story:

- Code is implemented behind static dependency injection using traits and generics. Do not introduce dynamic dispatch unless a later story explicitly approves it.
- Async work uses `tokio`.
- Errors use `thiserror`.
- New behavior has unit tests. After story 009 lands, behavior that crosses process or infrastructure boundaries should also have integration coverage in the dedicated integration test target.
- Seed data is updated whenever a story introduces user-visible or queryable data.
- The QA guide in the story can be followed from a clean checkout with local Docker dependencies.
