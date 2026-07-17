# Policies Epic

Add policies as a workspace-scoped part of the control domain so compliance
officers can manage policy records and one safely scanned document through MCP,
while auditors can browse the complete policy catalog and trace policies from
each control. The core principle is one policy record, many controls, and one
current attachment whose bytes never pass through MCP.

Full schema, lifecycle, MCP, attachment, and auditor decisions live in
[spec.md](./spec.md). Browser behavior lives in [ux.md](./ux.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Policy Domain And Persistence](./tickets/001-policy-domain-and-persistence.md) | Done | Added active policy lifecycle, mappings, and the attachment schema prerequisite. |
| 002. [MCP Policy Catalog Tools](./tickets/002-mcp-policy-catalog-tools.md) | Done | Added policy MCP tools with catalog types in the dedicated projections layer. |
| 003. [Policy Attachment Lifecycle](./tickets/003-policy-attachment-lifecycle.md) | Todo | Add one policy attachment and extend scan/finalization safely. |
| 004. [MCP Policy Attachment Grants](./tickets/004-mcp-policy-attachment-grants.md) | Todo | Issue and redeem scoped human-browser attachment sessions. |
| 005. [Policy Attachment Management Page](./tickets/005-policy-attachment-management-page.md) | Todo | Add the evidence-like upload, status, archive, and download UI. |
| 006. [Auditor Policy Read Model And Downloads](./tickets/006-auditor-policy-read-model-and-downloads.md) | Todo | Expose safe policy data and session-bound document downloads. |
| 007. [Auditor Policy Portal UI](./tickets/007-auditor-policy-portal-ui.md) | Todo | Add policy navigation, catalog/detail pages, and control sections. |
| 008. [Policy Guidance And Demo Data](./tickets/008-policy-guidance-and-demo-data.md) | Todo | Teach agents the workflow and seed a representative policy catalog. |

## Sequencing

- **001** is foundational for every policy read, mutation, and auditor view.
- **002** depends on 001 and can ship metadata and mapping management before
  document upload is available.
- **003** depends on 001 and can proceed in parallel with 002; it establishes
  the policy attachment lifecycle and worker support.
- **004** depends on 002 and 003 because its MCP tool authorizes an existing
  policy and delegates the attachment lifecycle.
- **005** depends on 004 and completes the compliance officer's browser flow.
- **006** depends on 001 and 003 and can proceed alongside 004-005 once policy
  attachment eligibility is stable.
- **007** depends on 006 and extends the existing server-rendered auditor UI.
- **008** follows the externally visible MCP and portal contracts in 002, 004,
  and 007 so guidance and demo data describe the finished workflow.
