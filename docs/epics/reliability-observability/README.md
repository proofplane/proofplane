# Reliability And Observability Epic

Make failures predictable and runtime behavior visible without leaking tenant
identifiers or secrets into metrics.

Full failure and metric contracts live in [spec.md](./spec.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 002. [Messaging And Storage Failures](./tickets/002-messaging-and-storage-failures.md) | Todo | API quarantine-write coverage plus existing worker finalization guarantees. |
| 003. [HTTP And Access Metrics](./tickets/003-http-and-access-metrics.md) | Todo | API traffic, auth, and readiness metrics. |
| 004. [Async Pipeline Metrics](./tickets/004-async-pipeline-metrics.md) | Todo | Outbox, worker, scanner, and storage metrics. |
| 005. [Structured Audit Logging](./tickets/005-structured-audit-logging.md) | Todo | Define fields, remove the dormant table, and configure the sink contract. |
| 006. [MCP Metrics](./tickets/006-mcp-metrics.md) | Todo | Complete instrumentation after the MCP runtime lands. |
| 007. [Evidence Lifecycle Audit Logs](./tickets/007-evidence-lifecycle-audit-logs.md) | Todo | Domain audit events for evidence submission, attachment, grant, scan, and finalization lifecycle transitions. |

## Sequencing

- **003** can start immediately; authorization is Postgres-sourced policy and
  has no separate dependency-failure ticket.
- **002** builds on existing failure tests and production adapters.
- **004** can proceed with current local pipeline behavior.
- **005** is the shared contract for audit logs emitted by domain epics.
- **006** follows MCP Server ticket 005 and completes operational MCP
  instrumentation.
- **007** follows 005 and Evidence Lifecycle Completion ticket 004; it is a
  prerequisite for MCP submission-write equivalence.
