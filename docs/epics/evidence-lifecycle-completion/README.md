# Evidence Lifecycle Completion Epic

Finish the user-visible evidence lifecycle while preserving one rule: only a
malware-scanned, finalized attachment is normal downloadable evidence.

Full rationale and contracts live in [spec.md](./spec.md), the source of
technical depth. Tickets below are lean handoff units that link into it.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Latest Submission API](./tickets/001-latest-submission-api.md) | Todo | Expose the repository query already implemented. |
| 002. [Finalized Attachment Download](./tickets/002-finalized-attachment-download.md) | Todo | Stream only eligible finalized attachment content. |
| 003. [Evidence Demo Seed](./tickets/003-evidence-demo-seed.md) | Todo | Seed a deterministic submission and filesystem object. |

## Sequencing

- **001** and **002** can proceed in parallel on the existing evidence model.
- **003** follows 002 so the seeded object is verifiable through the public
  download contract.
- Audit events are intentionally sequenced in the Audit Trail epic.
