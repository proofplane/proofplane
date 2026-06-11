# 002 - Messaging And Storage Failures

**Status:** Todo · **Depends on:** production-runtime-adapters/002, production-runtime-adapters/003 · **Spec:** [spec.md](../spec.md#failure-contracts)

**Summary** - Complete public-boundary failure coverage for outbox publishing,
attachment storage, scanner delivery, and production adapters.

**Acceptance criteria**

- [ ] Given transient publish failure, when the dequeuer retries after recovery,
  then the outbox row is eventually published once and removed.
- [ ] Given storage write/read failure through an attachment API, when requested,
  then a stable error is returned and database/object state remains consistent.
- [ ] Given scanner unavailability through final delivery, when the worker
  processes the message, then retry and terminal behavior follows the existing
  delivery contract.
- [ ] Given existing concrete worker rollback tests, when this ships, then they
  remain integration tests against Postgres.

**Tasks**

- [ ] Extend Pub/Sub interruption/recovery coverage.
- [ ] Add public attachment storage failure tests.
- [ ] Add production adapter authentication/unavailability tests.
- [ ] Reuse existing scanner/finalization fixtures and close uncovered paths.
