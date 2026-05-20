# Proofplane MVP Story Backlog

This directory is an ordered, file-based execution plan for the Proofplane MVP. Treat each markdown file as a story inside one long-running epic.

The sequence intentionally front-loads platform scaffolding before product features:

| Story | Status | Notes |
| --- | --- | --- |
| 001. [Repository and Crate Scaffold](./001-repository-and-workspace-scaffold.md) | Done | Single-package Rust scaffold, binaries, core modules, and Makefile are in place. Integration tests are deferred to 009. |
| 002. [Local Docker Compose Dependencies](./002-local-docker-compose-dependencies.md) | Done | Docker Compose covers Postgres and the Pub/Sub emulator; local config reserves `.local/storage` for future filesystem object storage. |
| 003. [Applicative Validation Framework](./003-applicative-validation-framework.md) | Done | Generic `Validation<T, E>` API and `validate!` macro are implemented and tested. |
| 004. [Configuration System](./004-configuration-system.md) | Done | Typed YAML config loads through `PROOFPLANE_CONFIG`, validates with story 003, and redacts secrets. |
| 005. [Error, Retry, and Result Extensions](./005-error-retry-and-result-extensions.md) | Done | Shared async retry helper is in place and concrete boundary errors use `thiserror`; retry logging/metrics are deferred until real runtime needs appear. |
| 006. [Observability Scaffold](./006-observability-scaffold.md) | Done | Structured logging initializes through `tracing_subscriber`; metrics are deferred until real runtime boundaries exist. |
| 007. [HTTP API Runtime Scaffold](./007-http-api-runtime-scaffold.md) | Done | Axum API runtime serves health, readiness, version, metrics, stable errors, and structured request logs. |
| 008. [Database Migrations and Seed Data](./008-database-migrations-and-seed-data.md) | Done | Refinery migrations, Postgres pool wiring, startup migrations, and idempotent local seed data are in place. |
| 009. [Integration Test Harness](./009-integration-test-harness.md) | Deferred | Not started. Introduce this when feature work needs process or infrastructure integration coverage. |
| 010. [Authentication, Actors, and Request Middleware](./010-authentication-actors-and-request-middleware.md) | Next | Not started. |
| 011. [Pub/Sub Client and Subscription Runtime](./011-pubsub-client-and-subscription-runtime.md) | Planned | Not started. |
| 012. [Transactional Outbox](./012-transactional-outbox.md) | Planned | Not started. |
| 013. [Worker Runtime and Outbox Dequeuer](./013-worker-runtime-and-outbox-dequeuer.md) | Planned | Not started. |
| 014. [GCS Object Storage Adapter](./014-gcs-object-storage-adapter.md) | Planned | Not started. |
| 015. [Evidence Requirements Domain](./015-evidence-requirements-domain.md) | Planned | Not started. |
| 016. [Controls and Requirement Mappings](./016-controls-and-requirement-mappings.md) | Planned | Not started. |
| 017. [Evidence Submissions and Attachments](./017-evidence-submissions-and-attachments.md) | Planned | Not started. |
| 018. [Submission Approval and Control Status](./018-submission-approval-and-control-status.md) | Planned | Not started. |
| 019. [Approved Source Material](./019-approved-source-material.md) | Planned | Not started. |
| 020. [Audit Log](./020-audit-log.md) | Planned | Not started. |
| 021. [MCP Server](./021-mcp-server.md) | Planned | Not started. |
| 022. [End-to-End Demo and Release Hardening](./022-end-to-end-demo-and-release-hardening.md) | Planned | Not started. |

## Parallelization Notes

The numbered order is the preferred integration order, but several stories can be developed in parallel once their shared prerequisites are done.

Shared gates:

- Stories 001-004 are complete and unblock the next layer of platform work.
- Story 005 standardized the retry helper and concrete `thiserror` boundary errors before deeper runtime work.
- Story 009 is deferred until feature work needs process or infrastructure integration coverage. No Postgres/testcontainers coverage has been pulled forward.

Near-term parallel lanes:

- Story 010 is the next mainline feature task.
- Story 009 should be added alongside the first feature or infrastructure boundary that needs end-to-end verification, with reusable Postgres, Pub/Sub, config, and binary helpers.

Infrastructure lanes after config:

- Story 011 and story 014 can proceed independently after 004, because Pub/Sub and object storage touch different modules.
- Story 012 depends on 008 for schema and should use 005/011 where appropriate, but its repository and state-machine work can be developed before the worker exists.
- Story 013 depends on 011 and 012, so it should wait for those interfaces to settle.

Product lanes:

- Story 010 depends on 007 and 008 because it needs API middleware and actor persistence.
- Story 015 depends on 004, 007, 008, and 010, then becomes the base for the main product model. Add story 009 before or alongside it if repository/API behavior needs integration coverage.
- Story 016 depends on 015.
- Story 017 depends on 014 and 015, and should also use 010 actor context.
- Story 018 depends on 016 and 017.
- Story 019 depends on the evidence/control/submission model from 015-018.
- Story 020 can start its schema/repository design after 008 and 010, but service-level audit calls should be integrated alongside stories 015-019.
- Story 021 depends on the domain services created by 015-020.
- Story 022 should remain last because it validates the complete system.

Definition of done for every story:

- Code is implemented behind static dependency injection using traits and generics. Do not introduce dynamic dispatch unless a later story explicitly approves it.
- Async work uses `tokio`.
- Errors use `thiserror`.
- New behavior has unit tests. After story 009 lands, behavior that crosses process or infrastructure boundaries should also have integration coverage in the dedicated integration test target.
- Seed data is updated whenever a story introduces user-visible or queryable data.
- The QA guide in the story can be followed from a clean checkout with local Docker dependencies.
