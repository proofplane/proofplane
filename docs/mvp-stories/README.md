# Proofplane MVP Story Backlog

This directory is an ordered, file-based execution plan for the Proofplane MVP. Treat each markdown file as a story inside one long-running epic.

The sequence intentionally front-loads platform scaffolding before product features:

| Story | Status | Notes |
| --- | --- | --- |
| 001. [Repository and Crate Scaffold](./001-repository-and-workspace-scaffold.md) | Done | Single-package Rust scaffold, binaries, modules, Makefile, and integration test target are in place. |
| 002. [Local Docker Compose Dependencies](./002-local-docker-compose-dependencies.md) | Done | Docker Compose covers Postgres and the Pub/Sub emulator; object storage uses the local filesystem backend. |
| 003. [Applicative Validation Framework](./003-applicative-validation-framework.md) | Done | Generic `Validation<T, E>` API and `validate!` macro are implemented and tested. |
| 004. [Configuration System](./004-configuration-system.md) | Next | Configuration parsing and validation should build on story 003. |
| 005. [Error, Retry, and Result Extensions](./005-error-retry-and-result-extensions.md) | Planned | Not started. |
| 006. [Observability Scaffold](./006-observability-scaffold.md) | Planned | Not started. |
| 007. [HTTP API Runtime Scaffold](./007-http-api-runtime-scaffold.md) | Planned | Not started. |
| 008. [Database Migrations and Seed Data](./008-database-migrations-and-seed-data.md) | Planned | Not started. |
| 009. [Integration Test Harness](./009-integration-test-harness.md) | Planned | Not started. |
| 010. [Authentication, Actors, and Request Middleware](./010-authentication-actors-and-request-middleware.md) | Planned | Not started. |
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

Definition of done for every story:

- Code is implemented behind static dependency injection using traits and generics. Do not introduce dynamic dispatch unless a later story explicitly approves it.
- Async work uses `tokio`.
- Errors use `thiserror`.
- New behavior has unit tests and, where it crosses process or infrastructure boundaries, integration tests in the dedicated integration test target under `tests/integration`.
- Seed data is updated whenever a story introduces user-visible or queryable data.
- The QA guide in the story can be followed from a clean checkout with local Docker dependencies.
