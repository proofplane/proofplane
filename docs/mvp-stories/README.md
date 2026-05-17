# Proofplane MVP Story Backlog

This directory is an ordered, file-based execution plan for the Proofplane MVP. Treat each markdown file as a story inside one long-running epic.

The sequence intentionally front-loads platform scaffolding before product features:

1. [Repository and Crate Scaffold](./001-repository-and-workspace-scaffold.md)
2. [Local Docker Compose Dependencies](./002-local-docker-compose-dependencies.md)
3. [Configuration System](./003-configuration-system.md)
4. [Applicative Validation Framework](./004-applicative-validation-framework.md)
5. [Error, Retry, and Result Extensions](./005-error-retry-and-result-extensions.md)
6. [Observability Scaffold](./006-observability-scaffold.md)
7. [HTTP API Runtime Scaffold](./007-http-api-runtime-scaffold.md)
8. [Database Migrations and Seed Data](./008-database-migrations-and-seed-data.md)
9. [Integration Test Harness](./009-integration-test-harness.md)
10. [Authentication, Actors, and Request Middleware](./010-authentication-actors-and-request-middleware.md)
11. [Pub/Sub Client and Subscription Runtime](./011-pubsub-client-and-subscription-runtime.md)
12. [Transactional Outbox](./012-transactional-outbox.md)
13. [Worker Runtime and Outbox Dequeuer](./013-worker-runtime-and-outbox-dequeuer.md)
14. [GCS Object Storage Adapter](./014-gcs-object-storage-adapter.md)
15. [Evidence Requirements Domain](./015-evidence-requirements-domain.md)
16. [Controls and Requirement Mappings](./016-controls-and-requirement-mappings.md)
17. [Evidence Submissions and Attachments](./017-evidence-submissions-and-attachments.md)
18. [Submission Approval and Control Status](./018-submission-approval-and-control-status.md)
19. [Approved Source Material](./019-approved-source-material.md)
20. [Audit Log](./020-audit-log.md)
21. [MCP Server](./021-mcp-server.md)
22. [End-to-End Demo and Release Hardening](./022-end-to-end-demo-and-release-hardening.md)

Definition of done for every story:

- Code is implemented behind static dependency injection using traits and generics. Do not introduce dynamic dispatch unless a later story explicitly approves it.
- Async work uses `tokio`.
- Errors use `thiserror`.
- New behavior has unit tests and, where it crosses process or infrastructure boundaries, integration tests in the dedicated integration test target under `tests/integration`.
- Seed data is updated whenever a story introduces user-visible or queryable data.
- The QA guide in the story can be followed from a clean checkout with local Docker dependencies.
