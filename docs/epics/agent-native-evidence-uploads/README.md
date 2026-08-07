# Agent-Native Evidence Uploads Epic

Enable agent runtimes to submit local evidence files without a human upload
page while keeping file bytes out of MCP and model context. Proofplane remains
the trusted ingestion boundary: it streams each file into quarantine, records
agent provenance, and starts the existing scan and finalization lifecycle.

Full rationale, contracts, schema, and decisions live in [spec.md](./spec.md),
the source of technical depth.

## Tracking

Tickets live on GitHub, not in this directory. This epic is complete.

- Epic: [#87 Epic: Agent-Native Evidence Uploads](https://github.com/proofplane/proofplane/issues/87)
- Tickets: attached to that issue as sub-issues, and labeled
  [`epic:agent-native-evidence-uploads`](https://github.com/proofplane/proofplane/issues?q=is%3Aissue+label%3Aepic%3Aagent-native-evidence-uploads)

The epic issue carries the ticket index, status, and sequencing. See
[`docs/agents/issue-tracker.md`](../../agents/issue-tracker.md) for the
workflow.
