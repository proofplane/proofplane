# Production Runtime Adapters Epic

Make the implemented asynchronous evidence pipeline deployable without changing
its domain behavior: local adapters stay simple, while production adapters use
managed Google Cloud services.

Full rationale and runtime decisions live in [spec.md](./spec.md).

## Tickets

| Ticket | Status | Notes |
| --- | --- | --- |
| 001. [Runtime Object Store](./tickets/001-runtime-object-store.md) | Todo | Remove filesystem-only service dependencies. |
| 002. [GCS Object Store](./tickets/002-gcs-object-store.md) | Todo | Implement the production storage contract. |
| 003. [Production Pub/Sub Startup](./tickets/003-production-pubsub-startup.md) | Todo | Remove the emulator-only dequeuer gate. |

## Sequencing

- **001** is foundational for 002 and can land without changing local behavior.
- **002** and **003** can proceed in parallel after their configuration
  contracts are confirmed.
- Release Hardening depends on all three tickets.
