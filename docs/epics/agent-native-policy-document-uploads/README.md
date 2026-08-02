# Agent-Native Policy Document Uploads Epic

Enable agent runtimes to upload local policy documents without a human upload
page while keeping bytes out of MCP and model context. The flow reuses
Proofplane's machine-transfer and document-processing machinery but preserves
the policy rule that only one current document exists and replacement is never
implicit.

Full rationale, schema, and decisions live in [spec.md](./spec.md), the source
of technical depth. Tickets below are lean handoff units that link into it.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Policy Machine Upload Grants](./tickets/001-policy-machine-upload-grants.md) | Done | Share transfer primitives and persist policy-specific authority. |
| 002. [Policy Document Streaming](./tickets/002-policy-document-streaming.md) | Done | Stream, atomically complete, replay, and resolve document races. |
| 003. [MCP Policy Upload Preparation](./tickets/003-mcp-policy-upload-preparation.md) | Done | Expose the trusted-runtime descriptor and polling guidance. |
| 004. [Policy Upload Operations](./tickets/004-policy-upload-operations.md) | Done | Add audit, metrics, end-to-end failures, and regression coverage. |

## Sequencing

- **001** establishes the grant aggregate, persistence, credential, and shared
  transfer boundaries without changing public upload behavior.
- **002** depends on 001 and delivers the raw endpoint plus single-winner
  policy document completion.
- **003** depends on 001 and 002 so the MCP tool only advertises a hardened
  end-to-end transfer contract.
- **004** follows 003 and verifies operability, scanner handoff, failure paths,
  and unchanged human management behavior.
