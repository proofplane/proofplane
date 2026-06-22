# Evidence Lifecycle Completion Epic

Finish the user-visible evidence lifecycle while preserving one rule: only a
malware-scanned, finalized attachment can receive a human download grant.

Full rationale and contracts live in [spec.md](./spec.md), the source of
technical depth. Tickets below are lean handoff units that link into it.

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Latest Submission API](./tickets/001-latest-submission-api.md) | Done | Latest authenticated submission detail API shipped and verified. |
| 002. [Attachment Download Grants](./tickets/002-attachment-download-grants.md) | Done | Stateless five-minute JWT URLs stream eligible attachments through Proofplane. |
| 003. [Evidence Demo Seed](./tickets/003-evidence-demo-seed.md) | Done | Deterministic local submission and filesystem-backed uploaded attachment seeded. |
| 004. [Evidence Submission Context](./tickets/004-evidence-submission-context.md) | Done | Compact create/latest and full direct-read response contracts shipped and verified. |

## Sequencing

- **001** and **002** can proceed in parallel on the existing evidence model.
- **003** follows 002 so the seeded object is verifiable through the grant and
  direct-download contract.
- **004** is independent of the download flow and extends the existing
  submission create/read contract.
