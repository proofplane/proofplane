# 004 - Async Pipeline Metrics

**Status:** Todo · **Depends on:** none · **Spec:** [spec.md](../spec.md#metrics-contract)

**Summary** - Instrument outbox, Pub/Sub, worker handlers, scanner, and object
storage so asynchronous evidence processing can be operated.

**Acceptance criteria**

- [ ] Given successful and failed deliveries, when metrics are read, then handler
  outcomes, duration, retry, and terminal counts are distinguishable.
- [ ] Given outbox activity, when metrics are read, then claim, publish, retry,
  and backlog signals are present.
- [ ] Given storage and scanner operations, when metrics are read, then coarse
  result and byte/duration signals are present without object keys or IDs.
- [ ] Given duplicate or stale messages, when handled, then they have an explicit
  low-cardinality outcome rather than being counted as failures.

**Tasks**

- [ ] Instrument dequeuer and publisher operations.
- [ ] Instrument worker dispatch and domain handlers.
- [ ] Instrument scanner and runtime object store.
- [ ] Add metric-contract tests around representative flows.
