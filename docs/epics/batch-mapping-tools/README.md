# Batch Mapping Tools Epic

Give MCP agents one-call fan-out over Proofplane's two many-to-many
relationships — evidence ↔ control and policy ↔ control — instead of a round trip
per pair. The shaping principle: **a batch goes one way**. Each tool fans out
from a single anchor entity to many counterparts, never a list on both sides, so
each n↔n relationship gets two tools covering one half each. Every batch is one
transaction: it applies completely or not at all.

Full rationale, SQL sketches, and the error contract live in
[spec.md](./spec.md) — the single source of technical depth.

## Tickets

| Ticket                                                                             | Status | Notes                                                    |
| ---------------------------------------------------------------------------------- | ------ | -------------------------------------------------------- |
| 001. [Batch Validation and Errors](./tickets/001-batch-validation-and-errors.md)    | Done   | Shared size/empty/duplicate checks and typed batch errors. |
| 002. [Map Evidence to Controls](./tickets/002-map-evidence-to-controls.md)          | Done   | One evidence → many controls, with per-pair rationale.     |
| 003. [Map Control to Evidence](./tickets/003-map-control-to-evidence.md)            | Done   | One control → many evidence, the mirror half.              |
| 004. [Unmap Evidence from Controls](./tickets/004-unmap-evidence-from-controls.md)  | Done   | One evidence → remove many control mappings.               |
| 005. [Unmap Control from Evidence](./tickets/005-unmap-control-from-evidence.md)    | Done   | One control → remove many evidence mappings.               |
| 006. [Attach Policy to Controls](./tickets/006-attach-policy-to-controls.md)        | Todo   | One policy → many controls, no rationale.                  |
| 007. [Attach Control to Policies](./tickets/007-attach-control-to-policies.md)      | Todo   | One control → many policies, the mirror half.              |
| 008. [Detach Policy and Control Batches](./tickets/008-detach-policy-control-batches.md) | Todo   | Both removal halves for policy ↔ control.             |
| 009. [Guide Documentation](./tickets/009-guide-documentation.md)                    | Todo   | Teach agents when to batch and that batches are atomic.    |

## Sequencing

- **001** is foundational: every other ticket validates batch shape and reports
  offending IDs through it. Land it first.
- **002** is the reference implementation — it carries per-pair rationale and the
  unknown-ID reporting pattern that 003–008 copy. Land it before the rest.
- **003**, **004**, and **005** depend on 002 for the established pattern but are
  independent of each other and can parallelize.
- **006** and **007** depend on 001 only; they can start alongside 003–005 once
  002 has set the pattern. **008** covers both policy removal halves in one
  ticket because, with no rationale and no per-pair payload, each half is a thin
  mirror of 006/007.
- **009** lands last, once the final tool names and error codes are settled.
