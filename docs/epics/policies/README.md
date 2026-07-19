# Policies Epic

Add policies as a workspace-scoped part of the control domain so compliance
officers can manage policy records and one safely scanned document through MCP,
while auditors can browse the complete policy catalog and trace policies from
each control. The core principle is one policy record, many controls, and one
current document whose bytes never pass through MCP.

Full schema, lifecycle, MCP, document, and auditor decisions live in
[spec.md](./spec.md). Browser behavior lives in [ux.md](./ux.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Policy Domain And Persistence](./tickets/001-policy-domain-and-persistence.md) | Done | Added active policy lifecycle and control mappings. |
| 002. [MCP Policy Catalog Tools](./tickets/002-mcp-policy-catalog-tools.md) | Done | Added policy MCP tools with catalog types in the dedicated projections layer. |
| 003. [Policy Document Lifecycle](./tickets/003-policy-document-lifecycle.md) | Done | Added required, context-derived creator attribution to shared document metadata. |
| 004. [MCP Policy Document Grants](./tickets/004-mcp-policy-document-grants.md) | Done | Added purpose-separated policy grants and browser sessions. |
| 005. [Policy Document Management Page](./tickets/005-policy-document-management-page.md) | Done | Added the evidence-like upload, status, archive, and download UI. |
| 006. [Auditor Policy Read Model And Downloads](./tickets/006-auditor-policy-read-model-and-downloads.md) | Done | Added safe policy composition and session-bound document downloads. |
| 007. [Auditor Policy Portal UI](./tickets/007-auditor-policy-portal-ui.md) | Done | Added policy navigation, catalog/detail pages, and control sections. |
| 008. [Policy Guidance And Demo Data](./tickets/008-policy-guidance-and-demo-data.md) | Done | Added agent guidance and deterministic document-less demo policies. |

## Sequencing

- **001** is foundational for every policy read, mutation, and auditor view.
- **002** depends on 001 and can ship metadata and mapping management before
  document upload is available.
- **003** depends on 001 and can proceed in parallel with 002; it establishes
  the policy document lifecycle and worker support.
- **004** depends on 002 and 003 because its MCP tool authorizes an existing
  policy and delegates the document lifecycle.
- **005** depends on 004 and completes the compliance officer's browser flow.
- **006** depends on 001 and 003 and can proceed alongside 004-005 once policy
  document eligibility is stable.
- **007** depends on 006 and extends the existing server-rendered auditor UI.
- **008** follows the externally visible MCP and portal contracts in 002, 004,
  and 007 so guidance and demo data describe the finished workflow.
