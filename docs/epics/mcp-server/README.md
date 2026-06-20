# MCP Server Epic

Make Proofplane a first-class agent backend while preserving the same services,
authorization, validation, and audit semantics used by REST.

Full runtime, tool, identity, and error decisions live in [spec.md](./spec.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [MCP Runtime And Authentication](./tickets/001-mcp-runtime-and-authentication.md) | Todo | Bind the server and establish user API-token context. |
| 002. [Compliance Read Tools](./tickets/002-compliance-read-tools.md) | Todo | Expose selectively detailed evidence, human attachment grants, controls, and packet preview. |
| 003. [Compliance Write Tools](./tickets/003-compliance-write-tools.md) | Todo | Expose submission and mapping writes. |
| 004. [Auditor Packet Export Tools](./tickets/004-auditor-packet-export-tools.md) | Todo | Request and monitor exports, then issue human download grants. |
| 005. [MCP Logging And Equivalence](./tickets/005-mcp-logging-and-equivalence.md) | Todo | Prove parity and attributable tool activity. |

## Sequencing

The API-token prerequisite in API Token And PASETO Migration ticket 006 and the
attachment-download prerequisite in Evidence Lifecycle Completion ticket 002
are already done.

Deliver the remaining work in these waves:

1. Start these independent foundations in parallel:
   - Evidence Lifecycle Completion
     [004](../evidence-lifecycle-completion/tickets/004-evidence-submission-context.md)
     adds bounded submission context.
   - Trusted Compliance Reads
     [003](../trusted-compliance-reads/tickets/003-evidence-freshness-and-usability.md)
     derives evidence readiness.
   - Reliability And Observability
     [005](../reliability-observability/tickets/005-structured-audit-logging.md)
     establishes the shared audit contract.
   - Production Runtime Adapters
     [001](../production-runtime-adapters/tickets/001-runtime-object-store.md)
     establishes the object-store abstraction used by the export worker.
   - MCP Server [001](./tickets/001-mcp-runtime-and-authentication.md) establishes
     the authenticated Streamable HTTP runtime.
2. After the relevant foundations:
   - Reliability And Observability
     [007](../reliability-observability/tickets/007-evidence-lifecycle-audit-logs.md)
     follows Evidence Lifecycle 004 and Reliability 005.
   - Trusted Compliance Reads
     [004](../trusted-compliance-reads/tickets/004-auditor-packet-preview.md)
     follows Evidence Lifecycle 004, Trusted Reads 003, and Reliability 005.
3. Build Trusted Compliance Reads
   [005](../trusted-compliance-reads/tickets/005-auditor-packet-export.md) after
   packet preview, the runtime object store, and the audit contract.
4. Add Trusted Compliance Reads
   [006](../trusted-compliance-reads/tickets/006-auditor-packet-download-grants.md)
   after export jobs are durable.
5. Build MCP Server [002](./tickets/002-compliance-read-tools.md),
   [003](./tickets/003-compliance-write-tools.md), and
   [004](./tickets/004-auditor-packet-export-tools.md) in parallel after their
   respective prerequisites and MCP Server 001 are done.
6. Finish the MCP epic with
   [005](./tickets/005-mcp-logging-and-equivalence.md), which proves REST/MCP
   parity and attributable tool activity.
7. Complete operational instrumentation with Reliability And Observability
   [006](../reliability-observability/tickets/006-mcp-metrics.md) after MCP
   Server 005.

The MCP Server epic is complete after wave 6. The production-operational MCP
MVP is complete after wave 7 plus Production Runtime Adapters tickets 002 and
003 for GCS and production Pub/Sub. ZIP transfer remains an HTTP grant download;
the agent only requests work, polls status, and presents the URL. The standalone
source-material API remains deferred.
