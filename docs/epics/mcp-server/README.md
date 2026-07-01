# MCP Server Epic

Make Proofplane a first-class agent backend while preserving the same services,
authorization, validation, and audit semantics used by REST.

Full runtime, tool, identity, and error decisions live in [spec.md](./spec.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [MCP Runtime And Authentication](./tickets/001-mcp-runtime-and-authentication.md) | Done | Authenticated Streamable HTTP runtime, public operations routes, and graceful shutdown shipped. |
| 002. [Compliance Read Tools](./tickets/002-compliance-read-tools.md) | Done | Core read tools expose selectively detailed evidence, human attachment grants, controls, and mappings. |
| 003. [Compliance Write Tools](./tickets/003-compliance-write-tools.md) | Done | Submission and mapping writes are exposed with REST-equivalent persistence and audit semantics. |
| 005. [MCP Logging And Equivalence](./tickets/005-mcp-logging-and-equivalence.md) | Todo | Prove parity and attributable tool activity. |

## Sequencing

The API-token prerequisite in API Token And PASETO Migration ticket 006 and all
Evidence Lifecycle Completion tickets are already done.

Deliver the remaining work in these waves:

1. Start these core-demo foundations in parallel:
   - Reliability And Observability
     [005](../reliability-observability/tickets/005-structured-audit-logging.md)
     establishes the shared audit contract.
   - MCP Server [001](./tickets/001-mcp-runtime-and-authentication.md) establishes
     the authenticated Streamable HTTP runtime.
2. Build MCP Server [002](./tickets/002-compliance-read-tools.md) after MCP
   Server 001. Complete Reliability And Observability
   [007](../reliability-observability/tickets/007-evidence-lifecycle-audit-logs.md)
   after Reliability 005, then build MCP Server
   [003](./tickets/003-compliance-write-tools.md).
3. At this point the core MCP evidence lifecycle is demoable. The agent can
   inspect due requests and latest evidence, create submissions, issue human
   attachment grants, and inspect or update control mappings.
4. Finish the MCP epic with
   [005](./tickets/005-mcp-logging-and-equivalence.md), which proves REST/MCP
   parity and attributable tool activity.
5. Build Auditor Portal Access separately. Its MCP tools create, list, and
   revoke auditor links; auditor review and attachment downloads remain browser
   workflows.
6. Complete operational instrumentation with Reliability And Observability
   [006](../reliability-observability/tickets/006-mcp-metrics.md) after MCP
   Server 005.

The core MCP demo milestone is wave 3. The MCP Server epic is complete after
wave 4. The production-operational MCP MVP is complete after wave 6 plus
Production Runtime Adapters tickets 002 and 003 for GCS and production Pub/Sub.
Auditor portal access remains a separate epic. The standalone source-material
API remains deferred.
