# Trusted Compliance Reads Epic

Turn controls and evidence into concise, provenance-bearing packet views for
agents and auditors. Trust comes from immutable summarized submissions,
finalized attachments, freshness, and record provenance rather than a hidden
approval flag.

Full schema, freshness, packet, and export decisions live in
[spec.md](./spec.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 003. [Evidence Freshness And Usability](./tickets/003-evidence-freshness-and-usability.md) | Todo | Derive current, stale, missing, and unusable states. |
| 004. [Auditor Packet Preview](./tickets/004-auditor-packet-preview.md) | Todo | Assemble the control-to-evidence read model. |
| 005. [Auditor Packet Export Jobs](./tickets/005-auditor-packet-export.md) | Todo | Build packet ZIPs asynchronously into object storage. |
| 006. [Auditor Packet Download Grants](./tickets/006-auditor-packet-download-grants.md) | Todo | Give humans short-lived browser URLs without agent-mediated bytes. |

## Sequencing

- This epic is the auditor-packet lane and does not block the core MCP demo.
- **003** depends on the completed latest-submission and attachment-eligibility
  contracts.
- **004** depends on Evidence Lifecycle Completion ticket 004 and 003.
- **005** depends on 004, the runtime object-store abstraction, the shared audit
  contract, and finalized attachment eligibility.
- **006** depends on 005. Production delivery additionally requires the GCS
  object store and production Pub/Sub startup tickets.

## Deferred Work

The standalone source-material model, curation API, lifecycle, and full-text
search are deferred until a concrete workflow requires narratives spanning
multiple controls, requests, and submissions. Evidence Submission summaries
cover the immediate need without a second provenance model.
