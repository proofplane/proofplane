# Trusted Compliance Reads Epic

Turn controls and evidence into concise, provenance-bearing reads for agents and
auditors. Trust comes from explicit curation, finalized attachments, freshness,
and audit history rather than a hidden approval flag.

Full schema, freshness, packet, and export decisions live in
[spec.md](./spec.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Source Material Model](./tickets/001-source-material-model.md) | Todo | Add schema, invariants, and persistence. |
| 002. [Source Material API](./tickets/002-source-material-api.md) | Todo | Create, update, retrieve, and search curated material. |
| 003. [Freshness And Usability](./tickets/003-freshness-and-usability.md) | Todo | Derive current, stale, unusable, and retired states. |
| 004. [Auditor Packet Preview](./tickets/004-auditor-packet-preview.md) | Todo | Assemble the control-to-evidence read model. |
| 005. [Auditor Packet Export](./tickets/005-auditor-packet-export.md) | Todo | Stream a ZIP with manifest, summary, and usable files. |

## Sequencing

- **001** precedes 002 and 003.
- **002** and **003** can overlap once the schema is stable.
- **004** depends on Evidence Lifecycle Completion and 003.
- **005** depends on 004 and finalized attachment download eligibility.
