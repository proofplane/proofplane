# Reliability And Observability Epic

Make failures predictable and runtime behavior visible without leaking tenant
identifiers or secrets into metrics.

Full failure and metric contracts live in [spec.md](./spec.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Postgres And Authorization Failures](./tickets/001-postgres-and-authorization-failures.md) | Todo | Readiness, recovery, and auth ordering. |
| 002. [Messaging And Storage Failures](./tickets/002-messaging-and-storage-failures.md) | Todo | Public-boundary Pub/Sub, scanner, and storage behavior. |
| 003. [HTTP And Access Metrics](./tickets/003-http-and-access-metrics.md) | Todo | API traffic, auth, and readiness metrics. |
| 004. [Async Pipeline Metrics](./tickets/004-async-pipeline-metrics.md) | Todo | Outbox, worker, scanner, and storage metrics. |
| 005. [Structured Audit Logging](./tickets/005-structured-audit-logging.md) | Todo | Define fields, remove the dormant table, and configure the sink contract. |
| 006. [MCP Metrics](./tickets/006-mcp-metrics.md) | Todo | Complete instrumentation after the MCP runtime lands. |

## Sequencing

- **001** and 003 can start immediately.
- **002** builds on existing failure tests and production adapters.
- **004** can proceed with current local pipeline behavior.
- **005** is the shared contract for audit logs emitted by domain epics.
- **006** follows the MCP Server runtime.
