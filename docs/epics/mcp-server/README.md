# MCP Server Epic

Make Proofplane a first-class agent backend while preserving the same services,
authorization, validation, and audit semantics used by REST.

Full runtime, tool, identity, and error decisions live in [spec.md](./spec.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [MCP Runtime And Authentication](./tickets/001-mcp-runtime-and-authentication.md) | Todo | Bind the server and establish user API-token context. |
| 002. [Compliance Read Tools](./tickets/002-compliance-read-tools.md) | Todo | Expose evidence, human download grants, controls, source material, and packet reads. |
| 003. [Compliance Write Tools](./tickets/003-compliance-write-tools.md) | Todo | Expose supported submission, mapping, and curation writes. |
| 004. [MCP Logging And Equivalence](./tickets/004-mcp-logging-and-equivalence.md) | Todo | Prove parity and attributable tool activity. |

## Sequencing

- **001** depends on API Token And PASETO Migration ticket 006.
- **002** follows 001 and Evidence Lifecycle Completion ticket 002, and can grow
  as Trusted Compliance Reads lands.
- **003** follows 001 and the relevant service contracts.
- **004** depends on the read/write set and the shared structured audit-log
  contract.
