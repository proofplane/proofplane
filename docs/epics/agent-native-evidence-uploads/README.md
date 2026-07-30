# Agent-Native Evidence Uploads Epic

Enable agent runtimes to submit local evidence files without a human upload
page while keeping file bytes out of MCP and model context. Proofplane remains
the trusted ingestion boundary: it streams each file into quarantine, records
agent provenance, and starts the existing scan and finalization lifecycle.

Full rationale, contracts, schema, and decisions live in
[spec.md](./spec.md), the source of technical depth.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Transport-Neutral Evidence Ingestion](./tickets/001-transport-neutral-evidence-ingestion.md) | Todo | Extract reusable streaming ingestion without changing browser uploads. |
| 002. [Machine Upload Grants](./tickets/002-machine-upload-grants.md) | Todo | Persist and issue one-file, agent-attributed upload authority. |
| 003. [Machine Streaming Endpoint](./tickets/003-machine-streaming-endpoint.md) | Todo | Accept raw HTTP streams and create pending submissions. |
| 004. [Idempotent Upload Completion](./tickets/004-idempotent-upload-completion.md) | Todo | Make retries, races, rollback, and cleanup deterministic. |
| 005. [MCP Upload Preparation](./tickets/005-mcp-upload-preparation.md) | Todo | Give agents a safe transfer descriptor and polling workflow. |
| 006. [Upload Operations And Guidance](./tickets/006-upload-operations-and-guidance.md) | Todo | Add audit events, metrics, documentation, and failure coverage. |

## Sequencing

- **001** and **002** can proceed in parallel: one establishes reusable
  ingestion, while the other establishes machine authority and persistence.
- **003** depends on 001 and 002 and delivers the first end-to-end machine
  upload.
- **004** follows 003 and hardens its completion boundary before agents are
  directed to depend on it.
- **005** depends on 002 through 004 and preserves the existing
  `manage_evidence_submissions` human workflow.
- **006** follows the end-to-end flow and builds on Reliability and
  Observability tickets 005 and 007, which are already complete.
