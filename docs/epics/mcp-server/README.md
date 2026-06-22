# MCP Server Epic

Make Proofplane a first-class agent backend while preserving the same services,
authorization, validation, and audit semantics used by REST.

Full runtime, tool, identity, and error decisions live in [spec.md](./spec.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [MCP Runtime And Authentication](./tickets/001-mcp-runtime-and-authentication.md) | Todo | Bind the server and establish user API-token context. |
| 002. [Compliance Read Tools](./tickets/002-compliance-read-tools.md) | Todo | Expose selectively detailed evidence, human attachment grants, controls, and mappings. |
| 003. [Compliance Write Tools](./tickets/003-compliance-write-tools.md) | Todo | Expose submission and mapping writes. |
| 004. [Auditor Packet Tools](./tickets/004-auditor-packet-tools.md) | Todo | Preview readiness, request and monitor exports, then issue human grants. |
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
4. Build the auditor-packet lane separately:
   - Trusted Compliance Reads
     [003](../trusted-compliance-reads/tickets/003-evidence-freshness-and-usability.md)
     derives packet readiness.
   - Trusted Compliance Reads
     [004](../trusted-compliance-reads/tickets/004-auditor-packet-preview.md)
     consumes those states in the compact preview.
   - Production Runtime Adapters
     [001](../production-runtime-adapters/tickets/001-runtime-object-store.md)
     enables worker-owned export storage.
   - Trusted Compliance Reads
     [005](../trusted-compliance-reads/tickets/005-auditor-packet-export.md) and
     [006](../trusted-compliance-reads/tickets/006-auditor-packet-download-grants.md)
     build and deliver the ZIP.
5. Add MCP Server [004](./tickets/004-auditor-packet-tools.md) after the packet
   services are stable.
6. Finish the MCP epic with
   [005](./tickets/005-mcp-logging-and-equivalence.md), which proves REST/MCP
   parity and attributable tool activity.
7. Complete operational instrumentation with Reliability And Observability
   [006](../reliability-observability/tickets/006-mcp-metrics.md) after MCP
   Server 005.

The core MCP demo milestone is wave 3. The MCP Server epic is complete after
wave 6. The production-operational MCP MVP is complete after wave 7 plus
Production Runtime Adapters tickets 002 and 003 for GCS and production Pub/Sub.
ZIP transfer remains an HTTP grant download; the agent only requests work,
polls status, and presents the URL. The standalone source-material API remains
deferred.
