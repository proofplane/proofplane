# Agent-Native Policy Document Uploads Epic

Enable agent runtimes to upload local policy documents without a human upload
page while keeping bytes out of MCP and model context. The flow reuses
Proofplane's machine-transfer and document-processing machinery but preserves
the policy rule that only one current document exists and replacement is never
implicit.

Full rationale, schema, and decisions live in [spec.md](./spec.md), the source
of technical depth.

## Tracking

Tickets live on GitHub, not in this directory. This epic is complete.

- Epic: [#88 Epic: Agent-Native Policy Document Uploads](https://github.com/proofplane/proofplane/issues/88)
- Tickets: attached to that issue as sub-issues, and labeled
  [`epic:agent-native-policy-document-uploads`](https://github.com/proofplane/proofplane/issues?q=is%3Aissue+label%3Aepic%3Aagent-native-policy-document-uploads)

The epic issue carries the ticket index, status, and sequencing. See
[`docs/agents/issue-tracker.md`](../../agents/issue-tracker.md) for the
workflow.
