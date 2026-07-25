# 002 - Messaging And Storage Failures

**Status:** Todo · **Depends on:** production-runtime-adapters/002, production-runtime-adapters/003 · **Spec:** [spec.md](../spec.md#failure-contracts)

**Summary** - Complete public-boundary failure coverage for outbox publishing,
the API-owned quarantine upload, scanner delivery, and production adapters.
Worker-owned finalization failures already have concrete integration-v2
coverage.

**Acceptance criteria**

- [ ] Given transient publish failure, when the dequeuer retries after recovery,
  then the outbox row is eventually published once and removed.
- [ ] Given the API cannot write an initial upload to quarantine storage, when
  the document request is handled, then a stable error is returned and no
  document row or scan-request outbox event is committed.
- [ ] Given the worker cannot copy a clean document to its final object key,
  when finalization is delivered, then the delivery remains retryable and the
  document remains `finalizing`.
- [ ] Given the final object was copied but the database transition fails, when
  finalization runs, then `Retryable` retries only the database update up to
  configured `worker.retry_attempts`.
- [ ] Given those local database retries are exhausted, when the handler
  returns, then Pub/Sub receives a retryable failure and the document remains
  `finalizing` rather than being incorrectly marked `failed`.
- [ ] Given final-object reads fail through a human download, when requested,
  then a stable error is returned without changing the document's persisted
  `uploaded` status.
- [ ] Given scanner unavailability through final delivery, when the worker
  processes the message, then retry and terminal behavior follows the existing
  delivery contract.
- [ ] Given existing concrete worker rollback tests, when this ships, then they
  remain integration-v2 tests against Postgres.

**Tasks**

- [ ] Extend Pub/Sub interruption/recovery coverage.
- [ ] Add the missing API quarantine-write failure integration-v2 test.
- [ ] Pass `worker.retry_attempts` into the finalization handler and use
  `Retryable::retry_with_attempts` around `mark_document_uploaded`.
- [ ] Test database-update success after a transient local retry and retryable
  handler failure after local retry exhaustion.
- [ ] Preserve the existing worker finalization copy-failure and best-effort
  quarantine-delete coverage.
- [ ] Add focused GCS credential/error-mapping tests without requiring local
  cloud access; keep real GCS success-path integration-v2 coverage in CI.
- [ ] Reuse existing scanner/finalization fixtures and close uncovered paths.

**Notes**

- The browser app does not report processing status in the MVP. Agents observe
  the existing document `upload_status` through compliance read tools.
- See the 2026-06-11 failure-contract revision in the spec.
